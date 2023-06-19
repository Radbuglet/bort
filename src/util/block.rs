use derive_where::derive_where;

use super::arena::{Arena, ArenaPtr, FreeListArena, SpecArena};

type BlockArena = FreeListArena;

pub type BlockRef<T> = ArenaPtr<Block<T>, BlockArena>;

const HAMMERED_OR_FULL_BLOCK_SLOT: usize = usize::MAX;

#[derive_where(Default)]
pub struct BlockAllocator<T> {
    blocks: Arena<Block<T>, BlockArena>,
    hammered: Option<BlockRef<T>>,
    non_full: Vec<BlockRef<T>>,
}

pub struct Block<T> {
    value: T,
    non_full_index: usize,
    occupied_mask: u128,
}

impl<T> BlockAllocator<T> {
    pub fn alloc(&mut self, block_ctor: impl FnOnce() -> T) -> BlockReservation<T> {
        let block = self
            .hammered
            .get_or_insert_with(|| match self.non_full.pop() {
                Some(block) => {
                    self.blocks.get_mut(&block).non_full_index = HAMMERED_OR_FULL_BLOCK_SLOT;
                    block
                }
                None => self.blocks.alloc(Block {
                    value: (block_ctor)(),
                    non_full_index: HAMMERED_OR_FULL_BLOCK_SLOT,
                    occupied_mask: u128::MAX,
                }),
            });

        let block_inner = self.blocks.get_mut(block);

        // Find the first open slot
        let slot_idx = block_inner.occupied_mask.trailing_ones();

        // Mark the slot as occupied
        block_inner.occupied_mask |= 1 << slot_idx;

        // If our mask if full, remove the block
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
        let block_data = self.blocks.get_mut(&reservation.block);

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
            let block_data = self.blocks.dealloc(reservation.block);

            // Update the perturbed index
            if let Some(perturbed) = self.non_full.get_mut(block_data.non_full_index) {
                self.blocks.get_mut(perturbed).non_full_index = block_data.non_full_index;
            }

            // Allow users to clean up their heap reservations
            block_dtor(block_data.value);
        }
    }

    pub fn block<'a>(&'a self, block: &'a BlockRef<T>) -> &'a T {
        &self.blocks.get(block).value
    }

    pub fn block_mut<'a>(&'a mut self, block: &'a BlockRef<T>) -> &'a mut T {
        &mut self.blocks.get_mut(block).value
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct BlockReservation<T> {
    pub block: BlockRef<T>,
    pub slot: usize,
}
