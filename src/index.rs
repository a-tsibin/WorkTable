use std::sync::Arc;

use crate::page::link::PageLink;
use crate::prelude::LockFreeSet;
use crate::WorkTableError;
use scc::TreeIndex;

pub trait TableIndex<Row> {
    fn save_row(&self, row: Row, link: PageLink) -> Result<(), WorkTableError>;

    fn delete_row(&self, row: Row, link: PageLink) -> Result<(), WorkTableError>;
}

impl<Row> TableIndex<Row> for () {
    fn save_row(&self, _: Row, _: PageLink) -> Result<(), WorkTableError> {
        Ok(())
    }

    fn delete_row(&self, _: Row, _: PageLink) -> Result<(), WorkTableError> {
        Ok(())
    }
}

pub enum IndexType<'a, T> {
    Unique(&'a TreeIndex<T, PageLink>),
    NonUnique(&'a TreeIndex<T, Arc<LockFreeSet<PageLink>>>),
}
