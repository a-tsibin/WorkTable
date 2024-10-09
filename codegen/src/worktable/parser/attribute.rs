use crate::worktable::parser::Parser;
use crate::worktable::Attributes;
use proc_macro2::TokenTree;
use syn::spanned::Spanned as _;

impl Parser {
    pub fn parse_attributes(&mut self) -> syn::Result<Attributes> {
        const ATTRS: &[&str] = &["name", "persist"];
        let mut attributes = Attributes::new();
        loop {
            let Some(ident) = self.input_iter.peek().cloned() else {
                break;
            };
            let TokenTree::Ident(ident) = ident else {
                return Err(syn::Error::new(
                    ident.span(),
                    "Expected field name identifier.",
                ));
            };
            if !ATTRS.contains(&ident.to_string().as_str()) {
                break;
            }
            let _ = self.input_iter.next();

            self.parse_colon()?;

            let name = self
                .input_iter
                .next()
                .ok_or(syn::Error::new(self.input.span(), "Expected token."))?;
            let name = if let TokenTree::Ident(name) = name {
                name
            } else {
                return Err(syn::Error::new(name.span(), "Expected identifier."));
            };
            attributes.insert(ident.to_string(), name.to_string());

            self.try_parse_comma()?;
        }

        Ok(attributes)
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use quote::quote;

    use crate::worktable::Parser;

    #[test]
    fn test_name_parse() {
        let tokens = TokenStream::from(quote! {name: TestName,});

        let mut parser = Parser::new(tokens);
        let name = parser.parse_attributes();

        assert!(name.is_ok());
        let name = name.unwrap();

        assert_eq!(name.iter().next().unwrap().1, "TestName");
    }

    #[test]
    fn test_empty() {
        let tokens = TokenStream::from(quote! {});

        let mut parser = Parser::new(tokens);
        let name = parser.parse_attributes();

        assert!(name.is_err());
    }

    #[test]
    fn test_literal_field() {
        let tokens = TokenStream::from(quote! {"nme": TestName,});

        let mut parser = Parser::new(tokens);
        let name = parser.parse_attributes();

        assert!(name.is_err());
    }

    #[test]
    fn test_wrong_field() {
        let tokens = TokenStream::from(quote! {nme: TestName,});

        let mut parser = Parser::new(tokens);
        let name = parser.parse_attributes();

        assert!(name.is_err());
    }

    #[test]
    fn test_no_comma() {
        let tokens = TokenStream::from(quote! {name: TestName});

        let mut parser = Parser::new(tokens);
        let name = parser.parse_attributes();

        assert!(name.is_err());
    }
}
