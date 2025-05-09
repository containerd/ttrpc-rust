use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::str;

// A struct that divide a name into serveral parts that meets rust's guidelines.
struct NameSpliter<'a> {
    name: &'a [u8],
    pos: usize,
}

impl<'a> NameSpliter<'a> {
    fn new(s: &str) -> NameSpliter {
        NameSpliter {
            name: s.as_bytes(),
            pos: 0,
        }
    }
}

impl<'a> Iterator for NameSpliter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.pos == self.name.len() {
            return None;
        }
        // skip all prefix '_'
        while self.pos < self.name.len() && self.name[self.pos] == b'_' {
            self.pos += 1;
        }
        let mut pos = self.name.len();
        let mut upper_len = 0;
        let mut meet_lower = false;
        for i in self.pos..self.name.len() {
            let c = self.name[i];
            if (b'A'..=b'Z').contains(&c) {
                if meet_lower {
                    // So it should be AaA or aaA
                    pos = i;
                    break;
                }
                upper_len += 1;
            } else if c == b'_' {
                pos = i;
                break;
            } else {
                meet_lower = true;
                if upper_len > 1 {
                    // So it should be AAa
                    pos = i - 1;
                    break;
                }
            }
        }
        let s = str::from_utf8(&self.name[self.pos..pos]).unwrap();
        self.pos = pos;
        Some(s)
    }
}

pub fn ttrpc_mod() -> TokenStream {
    let ttrpc = format_ident!("ttrpc");
    quote! { ::#ttrpc }
}

pub fn to_camel_case(name: &str) -> String {
    let mut camel_case_name = String::with_capacity(name.len());
    for s in NameSpliter::new(name) {
        let mut chs = s.chars();
        camel_case_name.extend(chs.next().unwrap().to_uppercase());
        camel_case_name.push_str(&s[1..].to_lowercase());
    }
    camel_case_name
}

/// Adjust method name to follow rust-guidelines.
pub fn to_snake_case(name: &str) -> String {
    let mut snake_method_name = String::with_capacity(name.len());
    for s in NameSpliter::new(name) {
        snake_method_name.push_str(&s.to_lowercase());
        snake_method_name.push('_');
    }
    snake_method_name.pop();
    snake_method_name
}

pub fn type_token(type_str: &str) -> TokenStream {
    if type_str == "()" {
        quote!(())
    } else {
        let idents: Vec<_> = type_str
            .split("::")
            .map(|ident| format_ident!("{}", ident))
            .collect();
        quote!( #(#idents)::* )
    }
}
