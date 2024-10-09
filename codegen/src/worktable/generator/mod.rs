mod index;
mod primary_key;
mod queries;
mod row;
mod table;
mod wrapper;

use crate::worktable::model::{Columns, PrimaryKey, Queries};
use crate::worktable::Attributes;
use proc_macro2::Ident;

pub struct Generator {
    pub name: Ident,
    pub attributes: Attributes,
    pub table_name: Option<Ident>,
    pub row_name: Option<Ident>,
    pub wrapper_name: Option<Ident>,
    pub index_name: Option<Ident>,
    pub pk: Option<PrimaryKey>,
    pub queries: Option<Queries>,
    pub columns: Columns,
}

impl Generator {
    pub fn new(name: Ident, columns: Columns) -> Self {
        Self {
            name,
            attributes: Attributes::default(),
            table_name: None,
            row_name: None,
            wrapper_name: None,
            index_name: None,
            pk: None,
            queries: None,
            columns,
        }
    }
}
