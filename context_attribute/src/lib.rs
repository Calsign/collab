use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;

// stolen eagerly from the context-attribute crate
// I wanted to be able to pass args instead of using doc comments though

#[proc_macro_attribute]
pub fn context(input_args: TokenStream, input: TokenStream) -> TokenStream {
    let parser = Punctuated::<syn::Expr, syn::Token![,]>::parse_separated_nonempty;
    let input_args = match parser.parse(input_args) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_compile_error().into(),
    };
    let input = syn::parse_macro_input!(input as syn::ItemFn);

    let vis = &input.vis;
    let sig = &input.sig;
    let body = &input.block.stmts;

    let output_type = match &input.sig.output {
        syn::ReturnType::Type(_, typ) => &*typ,
        syn::ReturnType::Default => {
            return quote_spanned! {
                input.sig.output.span() =>
                    compile_error!("function must have a return type");
            }
            .into();
        }
    };

    return quote! {
        #vis #sig {
            let res: #output_type =  (|| {
                #(#body)*
            })();
            return Ok(res.with_context(|| format!(#input_args))?);
        }
    }
    .into();
}
