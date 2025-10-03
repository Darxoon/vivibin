use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DataStruct, DeriveInput, Ident, Type};

struct NamedField<'a> {
    name: &'a Ident,
    ty: &'a Type,
}

impl<'a> NamedField<'a> {
    fn write_read_statement(&self, domain: &Ident, reader: &Ident) -> (Ident, TokenStream) {
        let NamedField { name, ty } = *self;
        
        let name = format_ident!("_{name}");
        let tokens = quote! {
            let #name: #ty = ::vivibin::ReadDomainExt::read_fallback::<#ty>(#domain, #reader)?;
        };
        
        (name, tokens)
    }
}

// TODO: tuple structs
#[allow(dead_code)]
enum Structure<'a> {
    Named(Vec<NamedField<'a>>),
    Tuple(Vec<&'a Type>),
}

impl<'a> Structure<'a> {
    fn from_syn_struct(data: &'a DataStruct) -> Self {
        let mut fields = Vec::new();
        
        for field in &data.fields {
            let field_name = field.ident
                    .as_ref().expect("Expected named field");
            
            let field_type = &field.ty;
            fields.push(NamedField {
                name: field_name,
                ty: field_type,
            });
        }
        
        Self::Named(fields)
    }
}

#[proc_macro_derive(Readable)]
pub fn derive_readable(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    let name = input.ident;
    
    let Data::Struct(data) = input.data else {
        panic!("Expected {name} to be a struct")
    };
    
    let structure = Structure::from_syn_struct(&data);
    
    let domain = Ident::new("domain", Span::call_site());
    let reader = Ident::new("reader", Span::call_site());
    
    let body = match &structure {
        Structure::Named(named_fields) => {
            let field_names = named_fields.iter()
                .map(|field| field.name)
                .collect::<Vec<_>>();
            
            let (var_names, statements) = named_fields.iter()
                .map(|field| field.write_read_statement(&domain, &reader))
                .unzip::<_, _, Vec<Ident>, Vec<TokenStream>>();
            
            quote! {
                #(#statements)*
                core::result::Result::Ok(#name {
                    #(#field_names: #var_names),*
                })
            }
        },
        Structure::Tuple(_) => todo!(),
    };
    
    return quote! {
        impl<D: ::vivibin::ReadDomain> ::vivibin::Readable<D> for Vec3 {
            fn from_reader<R: ::vivibin::Reader>(
                reader: &mut R,
                domain: D
            ) -> ::anyhow::Result<Self> {
                #body
            }
        }
    }.into();
}