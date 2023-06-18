use derive_where::derive_where;

use super::set::{FreeListHeap, LocalHeap, LocalHeapOf, LocalPtrOf};

type BlockHeap = FreeListHeap;
pub type BlockRef<T> = LocalPtrOf<BlockHeap, Block<T>>;

const HAMMERED_OR_FULL_BLOCK_SLOT: usize = usize::MAX;

#[derive_where(Default)]
pub struct BlockAllocator<T> {
    blocks: LocalHeapOf<BlockHeap, Block<T>>,
    hammered: Option<BlockRef<T>>,
    non_full: Vec<BlockRef<T>>,
}

pub struct Block<T> {
    value: T,
    free_index: usize,
    free_mask: u128,
}

impl<T> BlockAllocator<T> {
    pub fn alloc(&mut self, block_ctor: impl FnOnce() -> T) -> BlockReservation<T> {
        let block = self
            .hammered
            .get_or_insert_with(|| match self.non_full.pop() {
                Some(block) => {
                    self.blocks.get_mut(&block).free_index = HAMMERED_OR_FULL_BLOCK_SLOT;
                    block
                }
                None => self.blocks.alloc(Block {
                    value: (block_ctor)(),
                    free_index: HAMMERED_OR_FULL_BLOCK_SLOT,
                    free_mask: u128::MAX,
                }),
            });

        let block_inner = self.blocks.get_mut(block);

        // Find the first open slot
        let slot_idx = block_inner.free_mask.trailing_ones();

        // Mark the slot as occupied
        block_inner.free_mask |= 1 << slot_idx;

        // If our mask if full, remove the block
        let block_clone = block.clone();
        if block_inner.free_mask == u128::MAX {
            // N.B. `block` is already located in the `HAMMERED_OR_FULL_BLOCK_SLOT`.
            self.hammered = None;
        }

        BlockReservation {
            block: block_clone,
            slot: slot_idx as usize,
        }
    }

    pub fn dealloc(&mut self, block: BlockReservation<T>) {
        todo!();
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
