use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DataStruct, DeriveInput, Ident, Type};

struct NamedField<'a> {
    name: &'a Ident,
    ty: &'a Type,
    explicit_require_domain: bool,
}

impl<'a> NamedField<'a> {
    fn write_read_statement(&self, domain: &Ident, reader: &Ident, required_domain_impls: &[&Type]) -> (Ident, TokenStream) {
        let NamedField { name, ty, .. } = *self;
        
        let name_string = name.to_string();
        let name = format_ident!("_{}", name_string.strip_prefix("r#").unwrap_or(&name_string));
        
        // TODO: try getting away from extra-traits
        let explicit_read_impl = required_domain_impls.iter().copied()
            .any(|current| current == ty);
        
        let tokens = if explicit_read_impl {
            quote! {
                let #name: #ty = ::vivibin::CanRead::<#ty>::read(domain, reader)?;
            }
        } else {
            quote! {
                let #name: #ty = ::vivibin::ReadDomainExt::read_fallback::<#ty>(#domain, #reader)?;
            }
        };
        
        (name, tokens)
    }
    
    fn write_write_statement(&self, domain: &Ident, ctx: &Ident, required_domain_impls: &[&Type]) -> TokenStream {
        let NamedField { name, ty, .. } = *self;
        
        let explicit_write_impl = required_domain_impls.iter().copied()
            .any(|current| current == ty);
        
        if explicit_write_impl {
            quote! {
                ::vivibin::CanWrite::<#ty>::write(#domain, #ctx, &self.#name)?;
            }
        } else {
            quote! {
                ::vivibin::WriteDomainExt::write_fallback::<#ty>(#domain, #ctx, &self.#name)?;
            }
        }
    }
}

// TODO: tuple structs
#[allow(dead_code)]
enum Structure<'a> {
    Named(Vec<NamedField<'a>>),
    Tuple(Vec<&'a Type>),
}

impl<'a> Structure<'a> {
    fn required_domain_impls(&self) -> Vec<&Type> {
        match self {
            Structure::Named(named_fields) => {
                named_fields.iter()
                    .flat_map(|field| {
                        if field.explicit_require_domain {
                            Some(field.ty)
                        } else {
                            None
                        }
                    })
                    .collect()
            },
            _ => todo!(),
        }
    }
    
    fn field_names(&self) -> impl Iterator<Item = &Ident> {
        match self {
            Self::Named(named_fields) => {
                named_fields.iter()
                    .map(|field| field.name)
            },
            _ => todo!(),
        }
    }
    
    fn from_syn_struct(data: &'a DataStruct) -> Self {
        let mut fields = Vec::new();
        
        for field in &data.fields {
            let field_name = field.ident.as_ref().expect("Expected named field");
            
            let require_domain_ident = Ident::new("require_domain", Span::call_site());
            
            let mut explicit_require_domain = false;
            for attr in &field.attrs {
                if attr.path().get_ident().is_some_and(|ident| *ident == require_domain_ident) {
                    explicit_require_domain = true;
                }
            }
            
            let field_type = &field.ty;
            fields.push(NamedField {
                name: field_name,
                ty: field_type,
                explicit_require_domain,
            });
        }
        
        Self::Named(fields)
    }
}

#[proc_macro_derive(Readable, attributes(require_domain))]
pub fn derive_readable(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    let name = input.ident;
    
    let Data::Struct(data) = input.data else {
        panic!("Expected {name} to be a struct")
    };
    
    let structure = Structure::from_syn_struct(&data);
    
    let domain = Ident::new("domain", Span::call_site());
    let reader = Ident::new("reader", Span::call_site());
    
    let required_domain_impls: Vec<&Type> = structure.required_domain_impls();
    
    let body = match &structure {
        Structure::Named(named_fields) => {
            let field_names = structure.field_names();
            
            let (var_names, statements) = named_fields.iter()
                .map(|field| field.write_read_statement(&domain, &reader, &required_domain_impls))
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
    
    let constraint = if required_domain_impls.is_empty() {
        quote! { ::vivibin::ReadDomain }
    } else {
        quote! { #(::vivibin::CanRead<#required_domain_impls>)+* }
    };
    
    return quote! {
        impl<D: #constraint> ::vivibin::Readable<D> for #name {
            fn from_reader<R: ::vivibin::Reader>(
                reader: &mut R,
                domain: D
            ) -> ::anyhow::Result<Self> {
                #body
            }
        }
    }.into();
}

#[proc_macro_derive(Writable, attributes(require_domain))]
pub fn derive_writable(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    let name = input.ident;
    
    let Data::Struct(data) = input.data else {
        panic!("Expected {name} to be a struct")
    };
    
    let structure = Structure::from_syn_struct(&data);
    
    let domain = Ident::new("domain", Span::call_site());
    let reader = Ident::new("ctx", Span::call_site());
    
    let required_domain_impls: Vec<&Type> = structure.required_domain_impls();
    
    let body = match &structure {
        Structure::Named(named_fields) => {
            let statements = named_fields.iter()
                .map(|field| field.write_write_statement(&domain, &reader, &required_domain_impls))
                .collect::<Vec<_>>();
            
            quote! {
                #(#statements)*
            }
        },
        Structure::Tuple(_) => todo!(),
    };
    
    let constraint = if required_domain_impls.is_empty() {
        quote! { ::vivibin::WriteDomain }
    } else {
        quote! { #(::vivibin::CanWrite<#required_domain_impls>)+* }
    };
    
    return quote! {
        impl<D: #constraint> ::vivibin::Writable<D> for #name {
            fn to_writer(&self, ctx: &mut impl ::vivibin::WriteCtx, domain: &mut D) -> ::anyhow::Result<()> {
                #body
                Ok(())
            }
        }
    }.into();
}
