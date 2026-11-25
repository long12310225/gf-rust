//! `Accept-Language` negotiation for `ghttp` (ADR-138).
//!
//! GoFrame reads the request language from `context.Context`; the port has no
//! context, so [`middleware_accept_language`] stores the best advertised
//! language on [`Context`] and handlers pass it to `gi18n`'s
//! `translate_with_language`.

use gf_errors::gerror::Error;
use gf_i18n::gi18n::{DEFAULT_LANGUAGE, parse_accept_language};

use super::router::Context;

/// Sets `Context::language` from `Accept-Language`, defaulting to English.
pub fn middleware_accept_language(context: &mut Context) -> Result<(), Error> {
    let language = context
        .request
        .headers
        .get("Accept-Language")
        .and_then(|header| parse_accept_language(header).into_iter().next())
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_owned());
    context.language = language;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ghttp::message::{Header, Request};

    fn request(headers: &[(&str, &str)]) -> Request {
        let mut request = Request {
            method: "GET".to_owned(),
            path: "/".to_owned(),
            protocol: "HTTP/1.1".to_owned(),
            ..Request::default()
        };
        let mut parsed = Header::new();
        for (name, value) in headers {
            parsed.set(name, value);
        }
        request.headers = parsed;
        request
    }

    #[test]
    fn middleware_picks_the_best_language() {
        let mut context = Context::new(
            request(&[("Accept-Language", "fr-CH, fr;q=0.9, en;q=0.8")]),
            vec![Arc::new(middleware_accept_language)],
        );
        context.proceed().expect("chain");
        assert_eq!(context.language(), "fr-ch");

        let mut context = Context::new(
            request(&[("Accept-Language", "*")]),
            vec![Arc::new(middleware_accept_language)],
        );
        context.proceed().expect("chain");
        assert_eq!(context.language(), DEFAULT_LANGUAGE);

        let mut context = Context::new(request(&[]), vec![Arc::new(middleware_accept_language)]);
        context.proceed().expect("chain");
        assert_eq!(context.language(), DEFAULT_LANGUAGE);
    }

    #[test]
    fn accept_language_parsing_orders_by_quality() {
        let languages = parse_accept_language("en-US,en;q=0.9,fr;q=0.8,de;q=0");
        assert_eq!(languages, vec!["en-us", "en", "fr"]);
        assert!(parse_accept_language("").is_empty());
        assert!(parse_accept_language("*, ;q=0.5").is_empty());
    }
}
