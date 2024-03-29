use derive_where::derive_where;

use crate::util::arena::FreeingArena;

use super::arena::{AbaPtrFor, Arena, ArenaFor, FreeListArenaKind};

type BlockArena = FreeListArenaKind;

pub type BlockPtr<T> = AbaPtrFor<BlockArena, Block<T>>;

const HAMMERED_OR_FULL_BLOCK_SLOT: usize = usize::MAX;

#[derive(Debug)]
#[derive_where(Default)]
pub struct BlockAllocator<T> {
    blocks: ArenaFor<BlockArena, Block<T>>,
    hammered: Option<BlockPtr<T>>,
    non_full: Vec<BlockPtr<T>>,
}

#[derive(Debug)]
pub struct Block<T> {
    value: T,
    non_full_index: usize,
    occupied_mask: u128,
}

impl<T> BlockAllocator<T> {
    pub fn alloc(&mut self, block_ctor: impl FnOnce(usize) -> T) -> BlockReservation<T> {
        let block = self
            .hammered
            .get_or_insert_with(|| match self.non_full.pop() {
                Some(block) => {
                    self.blocks.get_aba_mut(&block).non_full_index = HAMMERED_OR_FULL_BLOCK_SLOT;
                    block
                }
                None => self.blocks.alloc_aba(Block {
                    value: block_ctor(128),
                    non_full_index: HAMMERED_OR_FULL_BLOCK_SLOT,
                    occupied_mask: 0,
                }),
            });

        let block_inner = self.blocks.get_aba_mut(block);

        // Find the first open slot
        let slot_idx = block_inner.occupied_mask.trailing_ones();

        // Mark the slot as occupied
        block_inner.occupied_mask |= 1 << slot_idx;

        // If our mask if full, remove the block

        #[allow(clippy::clone_on_copy)] // This allows us to more easily switch to an Rc-based arena
        let block_clone = block.clone();

        if block_inner.occupied_mask == u128::MAX {
            // N.B. `block` is already located in the `HAMMERED_OR_FULL_BLOCK_SLOT`.
            self.hammered = None;
        }

        BlockReservation {
            block: block_clone,
            slot: slot_idx as usize,
        }
    }

    pub fn dealloc(&mut self, reservation: BlockReservation<T>, block_dtor: impl FnOnce(T)) {
        let block_data = self.blocks.get_aba_mut(&reservation.block);

        // Remove the slot from the occupied bitset
        debug_assert_ne!(block_data.occupied_mask & (1 << reservation.slot), 0);

        let old_occupied_mask = block_data.occupied_mask;
        block_data.occupied_mask &= !(1 << reservation.slot);

        // If the block was full but no longer is, add it to the `non_full` list.
        if old_occupied_mask == u128::MAX {
            // In this case, the block is just full and non-hammered.
            debug_assert_eq!(block_data.non_full_index, HAMMERED_OR_FULL_BLOCK_SLOT);

            // Set the block's location
            block_data.non_full_index = self.non_full.len();

            // Push the block back into the `non_free_blocks` set
            self.non_full.push(reservation.block);
        }

        // If nothing is occupied and the block is not our "hammered" block, delete it!
        if block_data.occupied_mask == 0 && block_data.non_full_index != HAMMERED_OR_FULL_BLOCK_SLOT
        {
            let block_data = self.blocks.dealloc_aba(&reservation.block);

            // Remove from the `non_full` list
            self.non_full.swap_remove(block_data.non_full_index);

            if let Some(perturbed) = self.non_full.get_mut(block_data.non_full_index) {
                self.blocks.get_aba_mut(perturbed).non_full_index = block_data.non_full_index;
            }

            // Allow users to clean up their heap reservations
            block_dtor(block_data.value);
        }
    }

    pub fn block<'a>(&'a self, block: &'a BlockPtr<T>) -> &'a T {
        &self.blocks.get_aba(block).value
    }

    pub fn block_mut<'a>(&'a mut self, block: &'a BlockPtr<T>) -> &'a mut T {
        &mut self.blocks.get_aba_mut(block).value
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct BlockReservation<T> {
    pub block: BlockPtr<T>,
    pub slot: usize,
}
