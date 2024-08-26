use proc_macro2::TokenStream;
use quote::quote;

use gen_row_type::gen_row_def;
use gen_table_type::gen_table_def;
use parse_columns::parse_columns;
use parse_name::parse_name;

mod gen_row_type;
mod gen_table_type;
mod parse_columns;
mod parse_name;
mod parse_punct;

pub fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let mut i = input.clone().into_iter();

    let name = parse_name(&mut i, &input)?;
    let columns = parse_columns(&mut i, &input)?;

    let pk_type = columns
        .columns_map
        .get(&columns.primary_key)
        .expect("exists")
        .clone();

    let (row_def, row_ident) = gen_row_def(columns, name.clone());
    let table_def = gen_table_def(name, pk_type.to_string(), row_ident.to_string());

    Ok(TokenStream::from(quote! {
        #row_def

        #table_def
    }))
}