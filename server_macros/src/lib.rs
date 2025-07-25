use core::panic;

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Lit, Type, parse_macro_input};

#[proc_macro_derive(Packet, attributes(opcode))]
pub fn derive_packet(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let mut opcode = None;

    for attr in &input.attrs {
        if attr.path().is_ident("opcode") {
            match &attr.meta {
                syn::Meta::NameValue(meta) => {
                    match &meta.value {
                        Expr::Lit(lit) => match &lit.lit {
                            Lit::Int(val) => {
                                opcode = Some(val.base10_parse::<u8>().unwrap());
                            }
                            _ => {
                                panic!("Expected #[opcode = N]")
                            }
                        },
                        _ => {
                            panic!("Expected #[opcode = N]")
                        }
                    };
                }
                _ => {
                    panic!("Expected #[opcode = N]")
                }
            }
        }
    }

    // Deserialization logic
    let mut deserializers = Vec::new();
    let mut field_inits = Vec::new();

    if let Data::Struct(data_struct) = &input.data {
        if let Fields::Named(fields_named) = &data_struct.fields {
            for field in &fields_named.named {
                let field_name = field.ident.as_ref().unwrap();
                let ty = &field.ty;

                if let Some(ty_str) = type_ident_string(ty) {
                    match ty_str.as_str() {
                        "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" => {
                            if let Some(size) = int_byte_size(&ty_str) {
                                let buf_ident = syn::Ident::new(
                                    &format!("buf_{}", field_name),
                                    field_name.span(),
                                );
                                deserializers.push(quote! {
                                    let mut #buf_ident = [0u8; #size];
                                    reader.read_exact(&mut #buf_ident).await?;
                                    let #field_name = <#ty>::from_be_bytes(#buf_ident);
                                });
                                field_inits.push(quote! { #field_name });
                            }
                        }
                        "Vec" => {
                            if let Some(inner_ty) = extract_vec_inner_type(ty) {
                                if let Some(inner_ty_str) = type_ident_string(&inner_ty) {
                                    if let Some(size) = int_byte_size(&inner_ty_str) {
                                        let len_ident = syn::Ident::new(
                                            &format!("len_{}", field_name),
                                            field_name.span(),
                                        );
                                        let buf_ident = syn::Ident::new(
                                            &format!("buf_{}", field_name),
                                            field_name.span(),
                                        );
                                        let items_ident = syn::Ident::new(
                                            &format!("items_{}", field_name),
                                            field_name.span(),
                                        );

                                        deserializers.push(quote! {
                                            let mut #len_ident = [0u8; 1];
                                            reader.read_exact(&mut #len_ident).await?;
                                            let len = #len_ident[0] as usize;

                                            let mut #buf_ident = vec![0u8; len * #size];
                                            reader.read_exact(&mut #buf_ident).await?;

                                            let mut #items_ident = Vec::with_capacity(len);
                                            for chunk in #buf_ident.chunks_exact(#size) {
                                                let item = <#inner_ty>::from_be_bytes(chunk.try_into().unwrap());
                                                #items_ident.push(item);
                                            }

                                            let #field_name = #items_ident;
                                        });
                                        field_inits.push(quote! { #field_name });
                                    } else {
                                        panic!(
                                            "Vec<{}> is not a supported integer type",
                                            inner_ty_str
                                        );
                                    }
                                }
                            }
                        }
                        "String" => {
                            let len_ident =
                                syn::Ident::new(&format!("len_{}", field_name), field_name.span());
                            let buf_ident =
                                syn::Ident::new(&format!("buf_{}", field_name), field_name.span());

                            // Deserialize
                            deserializers.push(quote! {
                                let mut #len_ident = [0u8; 1];
                                reader.read_exact(&mut #len_ident).await?;
                                let len = #len_ident[0] as usize;
                                let mut #buf_ident = vec![0u8; len];
                                reader.read_exact(&mut #buf_ident).await?;
                                let #field_name = String::from_utf8(#buf_ident)
                                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                            });
                            field_inits.push(quote! { #field_name });
                        }
                        _ => {
                            panic!("This type {ty_str} is not parsable for a packet")
                        }
                    }
                }
            }
        }
    }

    let expanded = quote! {
        impl Packet for #name {
            const OPCODE: u8 = #opcode;

            // fn serialize(&self) -> Vec<u8> {
            //     let mut buffer = Vec::new();
            //     #(#serialize_fields)*
            //     buffer
            // }

            async fn deserialize<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> Result<Self, std::io::Error> {
                #(#deserializers)*
                Ok(Self {
                    #(#field_inits),*
                })
            }
        }
    };

    TokenStream::from(expanded)
}

fn type_ident_string(ty: &Type) -> Option<String> {
    if let Type::Path(p) = ty {
        let segment = p.path.segments.last()?;
        return Some(segment.ident.to_string());
    }
    None
}

fn int_byte_size(ty: &str) -> Option<usize> {
    match ty {
        "u8" | "i8" => Some(1),
        "u16" | "i16" => Some(2),
        "u32" | "i32" => Some(4),
        "u64" | "i64" => Some(8),
        _ => None,
    }
}

fn extract_vec_inner_type(ty: &Type) -> Option<Type> {
    if let Type::Path(type_path) = ty {
        let segment = type_path.path.segments.last()?;
        if segment.ident == "Vec" {
            if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                    return Some(inner_ty.clone());
                }
            }
        }
    }
    None
}
