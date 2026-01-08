use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, AngleBracketedGenericArguments, Data, DataStruct, DeriveInput,
    GenericArgument, Ident, Meta, PathArguments, Type, TypePath,
};

struct NamedField<'a> {
    name: &'a Ident,
    ty: &'a Type,
    explicit_require_domain: bool,
}

impl NamedField<'_> {
    fn write_read_statement(&self, domain: &Ident, reader: &Ident, vec_required: &mut bool, required_domain_impls: &[&Type]) -> (Ident, TokenStream) {
        let NamedField { name, ty, .. } = *self;
        
        let name_string = name.to_string();
        let name = format_ident!("_{}", name_string.strip_prefix("r#").unwrap_or(&name_string));
        
        let inner_vec_type = Self::get_vec_inner_type(ty);
        
        // TODO: try getting away from extra-traits
        let explicit_read_impl = required_domain_impls.iter().copied()
            .any(|current| current == ty);
        
        let tokens = match (inner_vec_type, explicit_read_impl) {
            (None, true) => quote! {
                let #name: #ty = ::vivibin::CanRead::<#ty>::read(#domain, #reader)?;
            },
            (None, false) => quote! {
                let #name: #ty = ::vivibin::Readable::from_reader(#reader, #domain)?;
            },
            (Some(inner_ty), true) => {
                *vec_required = true;
                quote! {
                    let #name: #ty = ::vivibin::ReadVecExt::read_std_vec::<#inner_ty, R>(#domain, #reader)?;
                }
            },
            (Some(inner_ty), false) => {
                *vec_required = true;
                quote! {
                    let #name: #ty = ::vivibin::ReadVecFallbackExt::read_std_vec_fallback::<#inner_ty, R>(#domain, #reader)?;
                }
            },
        };
        
        (name, tokens)
    }
    
    fn write_write_statement(&self, domain: &Ident, ctx: &Ident, cat: &Ident, vec_required: &mut bool, required_domain_impls: &[&Type]) -> TokenStream {
        let NamedField { name, ty, .. } = *self;
        
        let inner_vec_type = Self::get_vec_inner_type(ty);
        
        let explicit_write_impl = required_domain_impls.iter().copied()
            .any(|current| current == ty);
        
        match (inner_vec_type, explicit_write_impl) {
            (None, true) => quote! {
                ::vivibin::CanWrite::<#cat, #ty>::write(#domain, #ctx, &self.#name)?;
            },
            (None, false) => quote! {
                <#ty as ::vivibin::Writable<#cat, D>>::to_writer(&self.#name, #ctx, #domain)?;
            },
            (Some(inner_ty), true) => {
                *vec_required = true;
                quote! {
                    ::vivibin::WriteSliceExt::write_slice::<#inner_ty>(#domain, #ctx, &self.#name)?;
                }
            },
            (Some(inner_ty), false) => {
                *vec_required = true;
                quote! {
                    ::vivibin::WriteSliceFallbackExt::write_slice_fallback::<#inner_ty>(#domain, #ctx, &self.#name)?;
                }
            },
        }
    }
    
    fn get_vec_inner_type(ty: &Type) -> Option<&Type> {
        let Type::Path(TypePath { path, .. }) = ty else {
            return None;
        };
        
        let segments = &path.segments;
        if segments.last().is_none_or(|segment| segment.ident != "Vec") {
            return None;
        }
        
        let args = &segments.last().unwrap().arguments;
        let PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) = args else {
            return None;
        };
        
        if args.len() != 1 {
            return None;
        }
        
        if let GenericArgument::Type(inner_ty) = &args[0] {
            Some(inner_ty)
        } else {
            None
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
                    .filter_map(|field| {
                        field.explicit_require_domain.then_some(field.ty)
                    })
                    .collect()
            },
            Self::Tuple(_) => todo!(),
        }
    }
    
    fn field_names(&self) -> impl Iterator<Item = &Ident> {
        match self {
            Self::Named(named_fields) => {
                named_fields.iter()
                    .map(|field| field.name)
            },
            Self::Tuple(_) => todo!(),
        }
    }
    
    fn from_syn_struct(data: &'a DataStruct) -> Self {
        let mut fields = Vec::new();
        
        let boxed_ident = Ident::new("boxed", Span::call_site());
        let require_domain_ident = Ident::new("require_domain", Span::call_site());
        
        for field in &data.fields {
            let field_name = field.ident.as_ref().expect("Expected named field");
            
            
            let mut explicit_require_domain = false;
            for attr in &field.attrs {
                let Some(ident) = attr.path().get_ident() else {
                    continue;
                };
                
                if *ident == require_domain_ident {
                    explicit_require_domain = true;
                } else if *ident == boxed_ident {
                    panic!("#[boxed] attribute on a field is not supported yet!");
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

#[proc_macro_derive(Readable, attributes(require_domain, boxed, extra_read_domain_deps))]
pub fn derive_readable(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    let name = input.ident;
    
    let Data::Struct(data) = input.data else {
        panic!("Expected {name} to be a struct")
    };
    
    let boxed_ident = Ident::new("boxed", Span::call_site());
    let require_domain_ident = Ident::new("require_domain", Span::call_site());
    let extra_read_domain_deps_ident = Ident::new("extra_read_domain_deps", Span::call_site());
    
    let mut is_boxed = false;
    let mut extra_read_domain_deps = None;
    
    for attr in &input.attrs {
        let Some(ident) = attr.path().get_ident() else {
            continue;
        };
        
        if *ident == boxed_ident {
            is_boxed = true;
        } else if *ident == extra_read_domain_deps_ident {
            let Meta::List(list) = &attr.meta else {
                panic!("Expected arguments in #[extra_read_domain_deps(...)] attribute");
            };
            
            extra_read_domain_deps = Some(&list.tokens);
        } else if *ident == require_domain_ident {
            panic!("#[require_domain] attribute cannot be put on a type definition!");
        }
    }
    
    let structure = Structure::from_syn_struct(&data);
    
    let domain = Ident::new("domain", Span::call_site());
    let reader = Ident::new("reader", Span::call_site());
    
    let required_domain_impls: Vec<&Type> = structure.required_domain_impls();
    let mut vec_required = false;
    
    let body = match &structure {
        Structure::Named(named_fields) => {
            let field_names = structure.field_names();
            
            let (var_names, statements) = named_fields.iter()
                .map(|field| field.write_read_statement(&domain, &reader, &mut vec_required, &required_domain_impls))
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
    
    let constraint = match (required_domain_impls.is_empty(), vec_required) {
        (true, true) => quote! { ::vivibin::CanReadVec },
        (true, false) => quote! { ::vivibin::ReadDomain },
        (false, true) => quote! { ::vivibin::CanReadVec + #(::vivibin::CanRead<#required_domain_impls>)+* },
        (false, false) => quote! { #(::vivibin::CanRead<#required_domain_impls>)+* },
    };
    
    let extra_read_domain_deps = extra_read_domain_deps
        .map_or_else(TokenStream::new, |value| quote!(+ #value));
    
    let from_reader_def = if is_boxed {
        quote! {
            fn from_reader<R: ::vivibin::Reader>(reader: &mut R, domain: D) -> ::anyhow::Result<Self> {
                ::vivibin::ReadDomainExt::read_box(domain, reader, |reader| {
                    Self::from_reader_unboxed(reader, domain)
                })
            }
        }
    } else {
        quote! {}
    };
    
    quote! {
        impl<D: #constraint #extra_read_domain_deps> ::vivibin::Readable<D> for #name {
            fn from_reader_unboxed<R: ::vivibin::Reader>(
                reader: &mut R,
                domain: D
            ) -> ::anyhow::Result<Self> {
                #body
            }
            
            #from_reader_def
        }
    }.into()
}

#[proc_macro_derive(Writable, attributes(require_domain, extra_write_domain_deps))]
pub fn derive_writable(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    let name = input.ident;
    
    let Data::Struct(data) = input.data else {
        panic!("Expected {name} to be a struct")
    };
    
    let boxed_ident = Ident::new("boxed", Span::call_site());
    let require_domain_ident = Ident::new("require_domain", Span::call_site());
    let extra_write_domain_deps_ident = Ident::new("extra_write_domain_deps", Span::call_site());
    
    let mut extra_write_domain_deps = None;
    
    for attr in &input.attrs {
        let Some(ident) = attr.path().get_ident() else {
            continue;
        };
        
        if *ident == boxed_ident {
            // TODO: boxed serialization
        } else if *ident == extra_write_domain_deps_ident {
            let Meta::List(list) = &attr.meta else {
                panic!("Expected arguments in #[extra_write_domain_deps(...)] attribute");
            };
            
            extra_write_domain_deps = Some(&list.tokens);
        } else if *ident == require_domain_ident {
            panic!("#[require_domain] attribute cannot be put on a type definition!");
        }
    }
    
    let structure = Structure::from_syn_struct(&data);
    
    let domain = Ident::new("domain", Span::call_site());
    let reader = Ident::new("ctx", Span::call_site());
    
    let cat: Ident = Ident::new("Cat", Span::call_site());
    
    let required_domain_impls: Vec<&Type> = structure.required_domain_impls();
    let mut vec_required = false;
    
    let body = match &structure {
        Structure::Named(named_fields) => {
            let statements = named_fields.iter()
                .map(|field| field.write_write_statement(&domain, &reader, &cat, &mut vec_required, &required_domain_impls))
                .collect::<Vec<_>>();
            
            quote! {
                #(#statements)*
            }
        },
        Structure::Tuple(_) => todo!(),
    };
    
    let constraint = match (required_domain_impls.is_empty(), vec_required) {
        (true, true) => quote! { ::vivibin::CanWriteSlice<#cat> },
        (true, false) => quote! { ::vivibin::WriteDomain<Cat = #cat> },
        (false, true) => quote! { ::vivibin::CanWriteSlice<#cat> + #(::vivibin::CanWrite<#cat, #required_domain_impls>)+* },
        (false, false) => quote! { #(::vivibin::CanWrite<#cat, #required_domain_impls>)+* },
    };
    
    let extra_write_domain_deps = extra_write_domain_deps
        .map_or_else(TokenStream::new, |value| quote!(+ #value));
    
    quote! {
        impl<#cat: ::vivibin::HeapCategory, D: #constraint #extra_write_domain_deps> ::vivibin::Writable<#cat, D> for #name {
            fn to_writer_unboxed(&self, ctx: &mut impl ::vivibin::WriteCtx<#cat>, domain: &mut D) -> ::anyhow::Result<()> {
                #body
                Ok(())
            }
        }
    }.into()
}
