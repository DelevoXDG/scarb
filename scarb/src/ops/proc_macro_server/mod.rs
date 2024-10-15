use crate::compiler::plugin::proc_macro::{ExpansionKind, ProcMacroHost, ProcMacroInstance};
use anyhow::{anyhow, Result};
use cairo_lang_macro::{Diagnostic, TokenStream};
use connection::Connection;
use json_rpc::{ErrResponse, Method, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

mod connection;
mod json_rpc;
//mod plugin;

pub fn start_proc_macro_server(proc_macros: ProcMacroHost) -> Result<()> {
    let connection = Connection::new();
    let proc_macros = Arc::new(proc_macros);

    for _ in 0..4 {
        let receiver = connection.receiver.clone();
        let sender = connection.sender.clone();

        std::thread::spawn({
            let proc_macros = proc_macros.clone();

            move || {
                for request in receiver {
                    fn run_handler<M: Method>(
                        proc_macros: Arc<ProcMacroHost>,
                        value: Value,
                    ) -> Result<Value> {
                        M::handle(proc_macros.clone(), serde_json::from_value(value).unwrap())
                            .map(|res| serde_json::to_value(res).unwrap())
                    }

                    let response = match request.method.as_str() {
                        Expand::METHOD => run_handler::<Expand>(proc_macros.clone(), request.value),
                        ExpandInline::METHOD => {
                            run_handler::<ExpandInline>(proc_macros.clone(), request.value)
                        }
                        ExpandDerive::METHOD => {
                            run_handler::<ExpandDerive>(proc_macros.clone(), request.value)
                        }
                        DefinedInlineMacros::METHOD => {
                            run_handler::<DefinedInlineMacros>(proc_macros.clone(), request.value)
                        }
                        DefinedAttributes::METHOD => {
                            run_handler::<DefinedAttributes>(proc_macros.clone(), request.value)
                        }
                        DefinedExecutableAttributes::METHOD => run_handler::<
                            DefinedExecutableAttributes,
                        >(
                            proc_macros.clone(), request.value
                        ),
                        DefinedDerives::METHOD => {
                            run_handler::<DefinedDerives>(proc_macros.clone(), request.value)
                        }
                        _ => Err(anyhow!("method not found")),
                    };

                    let value = response.unwrap_or_else(|err| {
                        serde_json::to_value(ErrResponse::new(err.to_string())).unwrap()
                    });
                    let res = Response {
                        id: request.id,
                        value,
                    };

                    sender.send(res).unwrap();
                }
            }
        });
    }

    Ok(())
}

struct Expand;

#[derive(Deserialize)]
struct ExpandParams {
    r#macro: String,
    expansion_name: String,
    token_stream: TokenStream,
}

#[derive(Serialize)]
struct ExpandResult {
    token_stream: TokenStream,
    diagnostics: Vec<Diagnostic>,
}

fn handle(
    proc_macros: Arc<ProcMacroHost>,
    params: ExpandParams,
    kind: ExpansionKind,
) -> Result<ExpandResult> {
    let instance = proc_macros
        .macros()
        .into_iter()
        .find(|e| {
            e.get_expansions()
                .iter()
                .filter(|expansion| expansion.kind == kind)
                .any(|expansion| expansion.name == params.expansion_name)
        })
        .unwrap();

    let result = instance.generate_code(
        params.expansion_name.into(),
        TokenStream::new(params.r#macro),
        params.token_stream,
    );

    Ok(ExpandResult {
        token_stream: result.token_stream,
        diagnostics: result.diagnostics,
    })
}

impl Method for Expand {
    const METHOD: &'static str = "expand";

    type Params = ExpandParams;
    type Response = ExpandResult;

    fn handle(proc_macros: Arc<ProcMacroHost>, params: Self::Params) -> Result<Self::Response> {
        handle(proc_macros, params, ExpansionKind::Attr)
    }
}

struct ExpandInline;

impl Method for ExpandInline {
    const METHOD: &'static str = "expand-inline";

    type Params = ExpandParams;
    type Response = ExpandResult;

    fn handle(proc_macros: Arc<ProcMacroHost>, params: Self::Params) -> Result<Self::Response> {
        handle(proc_macros, params, ExpansionKind::Inline)
    }
}

struct ExpandDerive;

impl Method for ExpandDerive {
    const METHOD: &'static str = "expand-derive";

    type Params = ExpandParams;
    type Response = ExpandResult;

    fn handle(proc_macros: Arc<ProcMacroHost>, params: Self::Params) -> Result<Self::Response> {
        handle(proc_macros, params, ExpansionKind::Derive)
    }
}

fn defined<F, R>(proc_macros: Arc<ProcMacroHost>, mut map: F) -> Vec<String>
where
    F: FnMut(&ProcMacroInstance) -> R,
    R: IntoIterator<Item = String>,
{
    proc_macros
        .macros()
        .into_iter()
        .flat_map(|e| map(&*e))
        .collect()
}

macro_rules! defined {
    ($name:ident, $method:literal, $handler:expr) => {
        struct $name;

        impl Method for $name {
            const METHOD: &'static str = $method;

            type Params = ();
            type Response = Vec<String>;

            fn handle(
                proc_macros: Arc<ProcMacroHost>,
                _params: Self::Params,
            ) -> Result<Self::Response> {
                Ok(defined(proc_macros, $handler))
            }
        }
    };
}

defined!(DefinedInlineMacros, "defined-inline-macros", |e| e
    .inline_macros());
defined!(DefinedAttributes, "defined-attributes", |e| e
    .declared_attributes());
defined!(
    DefinedExecutableAttributes,
    "defined-executable-attributes",
    |e| e.executable_attributes()
);
defined!(DefinedDerives, "defined-derives", |e| e.declared_derives());
