use std::collections::HashMap;

use proc_macro2::{Ident, Literal, TokenStream};
use quote::__private::Span;
use quote::{quote, ToTokens};
use syn::ItemStruct;

use crate::name_generator::WorktableNameGenerator;

pub struct Generator {
    struct_def: ItemStruct,
    field_types: HashMap<Ident, TokenStream>,
}

impl WorktableNameGenerator {
    pub fn from_index_ident(index_ident: &Ident) -> Self {
        Self {
            name: index_ident.to_string().strip_suffix("Index").expect("index type nae should end on `Index`").to_string()
        }
    }

    pub fn get_persisted_index_ident(&self) -> Ident {
        Ident::new(
            format!("{}IndexPersisted", self.name).as_str(),
            Span::mixed_site(),
        )
    }
}

impl Generator {
    pub fn new(struct_def: ItemStruct) -> Self {
        Self {
            struct_def,
            field_types: HashMap::new(),
        }
    }

    /// Generates persisted index type. This type has same name as index, but with `Persisted` postfix. Field names of
    /// this type are same to index type, and values are `Vec<GeneralPage<IndexData<T>>>`, where `T` is index key type.
    pub fn gen_persist_type(&mut self) -> syn::Result<TokenStream> {
        let name_generator = WorktableNameGenerator::from_index_ident(&self.struct_def.ident);
        let name_ident = name_generator.get_persisted_index_ident();

        let mut fields = vec![];
        let mut types = vec![];

        for field in &self.struct_def.fields {
            fields.push(
                field
                    .ident
                    .clone()
                    .expect("index fields should always be named fields"),
            );
            let index_type = field.ty.to_token_stream().to_string();
            let mut split = index_type.split("<");
            // skip `TreeIndex` ident.
            split.next();
            let substr = split
                .next()
                .expect("index type should always contain key generic")
                .to_string();
            types.push(
                substr
                    .split(",")
                    .next()
                    .expect("index type should always contain key and value generics")
                    .to_string(),
            );
        }

        let fields: Vec<_> = fields
            .into_iter()
            .zip(types)
            .map(|(i, t)| {
                let t: TokenStream = t
                    .parse()
                    .expect("should be valid because parsed from declaration");
                self.field_types.insert(i.clone(), t.clone());
                quote! {
                    #i: Vec<GeneralPage<IndexData<#t>>>,
                }
            })
            .collect();

        Ok(quote! {
            #[derive(Debug, Default, Clone)]
            pub struct #name_ident {
                #(#fields)*
            }
        })
    }

    pub fn gen_persist_impl(&mut self) -> syn::Result<TokenStream> {
        let name_generator = WorktableNameGenerator::from_index_ident(&self.struct_def.ident);
        let name_ident = name_generator.get_persisted_index_ident();

        let get_intervals_fn = self.gen_get_intervals_fn();
        let persist_fn = self.gen_persist_fn();
        let parse_from_file_fn = self.gen_parse_from_file_fn();
        let gen_get_last_header_mut_fn = self.gen_get_last_header_mut_fn();

        Ok(quote! {
            impl #name_ident {
                #get_intervals_fn
                #persist_fn
                #gen_get_last_header_mut_fn
                #parse_from_file_fn
            }
        })
    }

    /// Generates `get_last_header_mut` function for persisted index. It checks all `Vec`s of pages and returns mutable
    /// header of last page.
    fn gen_get_last_header_mut_fn(&self) -> TokenStream {
        let get_last_header: Vec<_> = self
            .struct_def
            .fields
            .iter()
            .map(|f| f.ident.as_ref().expect("index fields should always be named fields"))
            .map(|i| {
                quote! {
                    if header.is_none() {
                        header = Some(
                            &mut self.#i
                                .last_mut()
                                .expect("at least one page should be presented, even if index contains no values")
                                .header
                        );
                    } else {
                        let new_header = &mut self.#i
                            .last_mut()
                            .expect("at least one page should be presented, even if index contains no values")
                            .header;
                        let header_page_id = header
                            .as_ref()
                            .expect("at least one page should be presented, even if index contains no values")
                            .page_id;
                        if header_page_id < new_header.page_id {
                            header = Some(new_header)
                        }
                    }
                }
            })
            .collect();

        quote! {
            pub fn get_last_header_mut(&mut self) -> Option<&mut GeneralHeader> {
                let mut header = None;
                #(#get_last_header)*
                header
            }
        }
    }

    /// Generates `persist` function for persisted index. It calls `persist_page` function for every page in index.
    fn gen_persist_fn(&self) -> TokenStream {
        let persist_logic = self
            .struct_def
            .fields
            .iter()
            .map(|f| {
                f.ident
                    .as_ref()
                    .expect("index fields should always be named fields")
            })
            .map(|i| {
                quote! {
                    for mut page in &mut self.#i {
                        persist_page(&mut page, file)?;
                    }
                }
            })
            .collect::<Vec<_>>();

        quote! {
            pub fn persist(&mut self, file: &mut std::fs::File) -> eyre::Result<()> {
                #(#persist_logic)*
                Ok(())
            }
        }
    }

    /// Generates `get_intervals` function for persisted index. It creates `HashMap` of index name, and it's page
    /// interval. Currently only one sequential `Interval` is returned for each index.
    fn gen_get_intervals_fn(&self) -> TokenStream {
        let interval_map_creation: Vec<_> = self
            .struct_def
            .fields
            .iter()
            .map(|f| {
                (
                    Literal::string(
                        f.ident
                            .as_ref()
                            .expect("index fields should always be named fields")
                            .to_string()
                            .as_str()
                    ),
                    f.ident.as_ref().expect("index fields should always be named fields"),
                )
            })
            .map(|(l, i)| {
                quote! {
                    let i = Interval (
                        self.#i
                            .first()
                            .expect("at least one page should be presented, even if index contains no values")
                            .header
                            .page_id
                            .into(),
                        self.#i
                            .last()
                            .expect("at least one page should be presented, even if index contains no values")
                            .header
                            .page_id
                            .into()
                    );
                    map.insert(#l.to_string(), vec![i]);
                }
            })
            .collect();

        quote! {
            pub fn get_intervals(&self) -> std::collections::HashMap<String, Vec<Interval>> {
                let mut map = std::collections::HashMap::new();
                #(#interval_map_creation)*
                map
            }
        }
    }

    /// Generates `parse_from_file` function for persisted index. It calls `parse_page` function for every page in each
    /// index interval and collects them into `Vec`'s. Then this `Vec`'s are used to construct persisted index object.
    fn gen_parse_from_file_fn(&self) -> TokenStream {
        let name_generator = WorktableNameGenerator::from_index_ident(&self.struct_def.ident);
        let page_const_name = name_generator.get_page_size_const_ident();

        let field_names_literals: Vec<_> = self
            .struct_def
            .fields
            .iter()
            .map(|f| (
                Literal::string(
                    f.ident
                        .as_ref()
                        .expect("index fields should always be named fields")
                        .to_string()
                        .as_str()
                ),
                f.ident
                    .as_ref()
                    .expect("index fields should always be named fields")
            ))
            .map(|(l, i)| quote! {
                let mut #i = vec![];
                let intervals = map.get(#l).expect("index name should exist");
                for interval in intervals {
                    for page_id in interval.0..interval.1 {
                        let index = parse_page::<IndexData<_>, { #page_const_name as u32 }>(file, page_id as u32)?;
                        #i.push(index);
                    }
                    let index = parse_page::<IndexData<_>, { #page_const_name as u32 }>(file, interval.1 as u32)?;
                    #i.push(index);
                }
            })
            .collect();

        let idents = self
            .struct_def
            .fields
            .iter()
            .map(|f| {
                f.ident
                    .as_ref()
                    .expect("index fields should always be named fields")
            })
            .collect::<Vec<_>>();

        quote! {
            pub fn parse_from_file(file: &mut std::fs::File, map: &std::collections::HashMap<String, Vec<Interval>>) -> eyre::Result<Self> {
                #(#field_names_literals)*

                Ok(Self {
                    #(#idents,)*
                })
            }
        }
    }

    /// Generates `PersistableIndex` trait implementation for persisted index.
    pub fn gen_persistable_impl(&self) -> syn::Result<TokenStream> {
        let ident = &self.struct_def.ident;
        let name_generator = WorktableNameGenerator::from_index_ident(&self.struct_def.ident);
        let name_ident = name_generator.get_persisted_index_ident();

        let get_index_names_fn = self.gen_get_index_names_fn();
        let get_persisted_index_fn = self.gen_get_persisted_index_fn();
        let from_persisted_fn = self.gen_from_persisted_fn()?;

        Ok(quote! {
            impl PersistableIndex for #ident {
                type PersistedIndex = #name_ident;

                #get_index_names_fn
                #get_persisted_index_fn
                #from_persisted_fn
            }
        })
    }

    /// Generates `get_index_names` function of `PersistableIndex` trait for persisted index. It just returns names of
    /// all indexes as strings.
    fn gen_get_index_names_fn(&self) -> TokenStream {
        let field_names_lits: Vec<_> = self
            .struct_def
            .fields
            .iter()
            .map(|f| {
                Literal::string(
                    f.ident
                        .as_ref()
                        .expect("index fields should always be named fields")
                        .to_string()
                        .as_str(),
                )
            })
            .map(|l| quote! { #l, })
            .collect();

        quote! {
            fn get_index_names(&self) -> Vec<&str> {
                vec![#(#field_names_lits)*]
            }
        }
    }

    /// Generates `get_persisted_index` function of `PersistableIndex` trait for persisted index. It maps every
    /// `TreeIndex` into `Vec` of `IndexPage`s using `map_tree_index`/`map_unique_tree_index` functions.
    fn gen_get_persisted_index_fn(&self) -> TokenStream {
        let name_generator = WorktableNameGenerator::from_index_ident(&self.struct_def.ident);
        let const_name = name_generator.get_page_size_const_ident();
        let idents = self
            .struct_def
            .fields
            .iter()
            .map(|f| {
                f.ident
                    .as_ref()
                    .expect("index fields should always be named fields")
            })
            .collect::<Vec<_>>();
        let field_names_init: Vec<_> = self
            .struct_def
            .fields
            .iter()
            .map(|f| {
                (
                    f.ident.as_ref().expect("index fields should always be named fields"),
                    !f.ty
                        .to_token_stream()
                        .to_string()
                        .to_lowercase()
                        .contains("lockfree"),
                )
            })
            .map(|(i, is_unique)| {
                let ty = self.field_types.get(i).expect("should be available as constructed from same values");
                if is_unique {
                    quote! {
                        let mut #i = map_index_pages_to_general(
                            map_unique_tree_index::<#ty, #const_name>(&self.#i),
                            previous_header
                        );
                        previous_header = &mut #i.last_mut()
                            .expect("at least one page should be presented, even if index contains no values")
                            .header;
                    }
                } else {
                    quote! {
                        let mut #i =  map_index_pages_to_general(
                            map_tree_index::<#ty, #const_name>(&self.#i),
                            previous_header
                        );
                        previous_header = &mut #i.last_mut()
                            .expect("at least one page should be presented, even if index contains no values")
                            .header;
                    }
                }
            })
            .collect();

        quote! {
            fn get_persisted_index(&self, header: &mut GeneralHeader) -> Self::PersistedIndex {
                let mut previous_header = header;
                #(#field_names_init)*
                Self::PersistedIndex {
                    #(#idents,)*
                }
            }
        }
    }

    /// Generates `from_persisted` function of `PersistableIndex` trait for persisted index. It maps every page in
    /// persisted page back to `TreeIndex`
    fn gen_from_persisted_fn(&self) -> syn::Result<TokenStream> {
        let idents = self
            .struct_def
            .fields
            .iter()
            .map(|f| {
                f.ident
                    .as_ref()
                    .expect("index fields should always be named fields")
            })
            .collect::<Vec<_>>();
        let index_gen = self
            .struct_def
            .fields
            .iter()
            .map(|f| {
                let i = f
                    .ident
                    .as_ref()
                    .expect("index fields should always be named fields");
                let is_unique = !f
                    .ty
                    .to_token_stream()
                    .to_string()
                    .to_lowercase()
                    .contains("lockfree");
                if is_unique {
                    quote! {
                        let #i = TreeIndex::new();
                        for page in persisted.#i {
                            page.inner.append_to_unique_tree_index(&#i);
                        }
                    }
                } else {
                    quote! {
                        let #i = TreeIndex::new();
                        for page in persisted.#i {
                            page.inner.append_to_tree_index(&#i);
                        }
                    }
                }
            })
            .collect::<Vec<_>>();

        Ok(quote! {
            fn from_persisted(persisted: Self::PersistedIndex) -> Self {
                #(#index_gen)*

                Self {
                    #(#idents,)*
                }
            }
        })
    }
}
