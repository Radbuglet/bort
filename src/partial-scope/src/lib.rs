use proc_macro::{Group, Span, TokenStream, TokenTree};
use std::{collections::HashSet, str::FromStr};

#[proc_macro]
pub fn partial_shadow(inp: TokenStream) -> TokenStream {
    let mut inp = inp.into_iter().peekable();

    // Define helpers
    fn visit_tokens(stream: TokenStream, f: &mut impl FnMut(&mut TokenTree)) -> TokenStream {
        stream
            .into_iter()
            .map(|mut token| {
                if let TokenTree::Group(group) = token {
                    let new_stream = visit_tokens(group.stream(), f);
                    let mut new_group = Group::new(group.delimiter(), new_stream);
                    new_group.set_span(group.span());
                    token = TokenTree::Group(new_group);
                }

                f(&mut token);

                token
            })
            .collect()
    }

    // Determine the list of variables not to shadow.
    let mut do_not_shadow = HashSet::new();

    #[allow(clippy::while_let_loop)]
    loop {
        // Consume ident
        if let Some(TokenTree::Ident(ident)) = inp.peek() {
            do_not_shadow.insert(ident.to_string());
            inp.next();
        } else {
            break;
        }

        // Consume delimiter
        match inp.peek() {
            Some(TokenTree::Punct(punct)) if punct.as_char() == ',' => {
                inp.next();
            }
            _ => break,
        }
    }

    // Consume semicolon
    match inp.peek() {
        Some(TokenTree::Punct(punct)) if punct.as_char() == ';' => {
            inp.next();
        }
        _ => {
            let span = inp.peek().map_or(Span::call_site(), |t| t.span());
            let err = TokenStream::from_str(r#"::core::compile_error!("Expected ';'");"#).unwrap();
            let err = visit_tokens(err, &mut |token| token.set_span(span));
            return err;
        }
    }

    // Process remaining tokens
    visit_tokens(inp.collect(), &mut |token| {
        if let TokenTree::Ident(ident) = token {
            if do_not_shadow.contains(&ident.to_string()) {
                ident.set_span(ident.span().resolved_at(Span::mixed_site()));
            }
        }
    })
}
