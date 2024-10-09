use crate::compiler::plugin::proc_macro::ProcMacroHostPlugin;
use anyhow::Result;
use cairo_lang_defs::{
    db::DefsGroup,
    plugin::{MacroPluginMetadata, PluginGeneratedFile, PluginResult},
};
use cairo_lang_filesystem::{
    cfg::CfgSet,
    ids::{FileKind, FileLongId, VirtualFile},
};
use cairo_lang_parser::db::ParserGroup;
use cairo_lang_semantic::plugin::PluginSuite;
use cairo_lang_syntax::node::{
    ast::{ExprInlineMacro, ModuleItem, ModuleItemList},
    kind::SyntaxKind,
    TypedSyntaxNode,
};
use cairo_lang_utils::{Intern, Upcast};
use db::ProcMacroServerDatabase;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    io::{BufRead, Write},
    sync::Arc,
    thread::JoinHandle,
};

mod db;

struct IoThreads {
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
}

impl Drop for IoThreads {
    fn drop(&mut self) {
        self.reader.join();
        self.writer.join();
    }
}

pub fn start_proc_macro_server(suite: PluginSuite) -> Result<()> {
    let (sender, receiver) = crossbeam_channel::bounded::<Request>(0);
    let (sender2, receiver2) = crossbeam_channel::bounded::<Response>(0);

    let reader = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut stdin = stdin.lock();

        let mut line = String::new();
        stdin.read_line(&mut line).unwrap();

        sender.send(serde_json::from_str(&line).unwrap()).unwrap();
    });

    let writer = std::thread::spawn(move || {
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();

        for response in receiver2 {
            let res = serde_json::to_vec(&response).unwrap();

            stdout.write_all(&res).unwrap()
        }
    });

    let io_threads = IoThreads { reader, writer };

    for _ in 0..4 {
        let receiver = receiver.clone();
        let sender = sender2.clone();

        std::thread::spawn(move || {
            for request in receiver {
                let response = match request.method {
                    Expand::METHOD => {
                        Expand::handle(plugin, serde_json::from_value(request.value).unwrap())
                            .and_then(serde_json::to_value)
                    }
                    _ => {}
                };

                match response {
                    Ok(res) => sender.send(res),
                    Err(err) => sender.send(ErrResponse::new(err)),
                };
            }
        });
    }

    Ok(())
}

#[derive(Serialize)]
struct Response {
    id: u64,
    value: serde_json::Value,
}

#[derive(Deserialize)]
struct Request {
    id: u64,
    method: String,
    value: serde_json::Value,
}

trait Method {
    const METHOD: &str;

    type Params: DeserializeOwned;
    type Response: Serialize;

    fn handle(plugin_suite: PluginSuite, params: Self::Params) -> Result<Self::Response>;
}

struct Expand;

#[derive(Deserialize)]
struct ExpandParams {
    code: String,
    metadata: MacroPluginMetadata<'static>,
}

impl Method for Expand {
    const METHOD: &'static str = "expand";

    type Params = ExpandParams;
    type Response = PluginResult;

    fn handle(plugin_suite: PluginSuite, params: Self::Params) -> Result<Self::Response> {
        let metadata = params.metadata;

        let (db, module_item) = parse(
            plugin_suite,
            params.code.to_owned(),
            Arc::new(metadata.cfg_set.clone()),
        );

        let plugins = db.macro_plugins();
        let result = plugins
            .first()
            .unwrap()
            .generate_code(db.upcast(), module_item, &metadata);

        Ok(result)
    }
}

struct ExpandInline;

impl Method for ExpandInline {
    const METHOD: &'static str = "expand-inline";

    type Params = ExpandParams;
    type Response = PluginResult;

    fn handle(plugin_suite: PluginSuite, params: Self::Params) -> Result<Self::Response> {
        let metadata = params.metadata;

        let (db, module_item) = parse(
            plugin_suite,
            params.code.to_owned(),
            Arc::new(metadata.cfg_set.clone()),
        );

        let plugins = db.macro_plugins();
        let result = plugins
            .first()
            .unwrap()
            .generate_code(db.upcast(), module_item, &metadata);

        Ok(result)
    }
}

impl ProcMacroHostPlugin {
    async fn expand_inline(
        &self,
        req: Request<ExpandInlineParams>,
    ) -> Result<Response<ExpandInlineResponse>, Status> {
        let req = req.get_ref().clone();
        let metadata = MetadataDataHolder::new(req.metadata);

        let (db, inline_macro) = self.parse_expr(req.code.to_owned(), metadata.cfg_set());

        let name = inline_macro
            .path(&db)
            .as_syntax_node()
            .get_text_without_trivia(&db);

        let plugins = db.inline_macro_plugins();
        let plugin = plugins.get(&name).unwrap();

        let result = plugin.generate_code(&db, &inline_macro, &metadata.macro_plugin_metadata());

        Ok(Response::new(ExpandInlineResponse {
            code: result.code.map(plugin_generated_file),
            diagnostics: result
                .diagnostics
                .into_iter()
                .map(|d| plugin_diagnostic(d, &db))
                .collect(),
        }))
    }

    async fn defined_inline_macros(
        &self,
        _: Request<Empty>,
    ) -> Result<Response<DefinedInlineMacrosResponse>, Status> {
        Ok(Response::new(DefinedInlineMacrosResponse {
            macros: ProcMacroServerDatabase::new(plugin_suite, Default::default())
                .inline_macro_plugins()
                .keys()
                .cloned()
                .collect(),
        }))
    }

    async fn defined_attributes(
        &self,
        _: Request<Empty>,
    ) -> Result<Response<DefinedAttributesResponse>, Status> {
        Ok(Response::new(DefinedAttributesResponse {
            // Not db.allowed_attributes() because we want to send only proc macros attributes and not builtin ones.
            attributes: ProcMacroServerDatabase::new(plugin_suite, Default::default())
                .macro_plugins()
                .into_iter()
                .flat_map(|plugin| plugin.declared_attributes())
                .collect(),
        }))
    }
}

fn parse(
    plugin_suite: PluginSuite,
    code: String,
    cfg_set: Arc<CfgSet>,
) -> (ProcMacroServerDatabase, ModuleItem) {
    let db = ProcMacroServerDatabase::new(plugin_suite, cfg_set);
    let file = FileLongId::Virtual(VirtualFile {
        parent: None,
        name: "parser_input".into(),
        content: code.into(),
        code_mappings: [].into(),
        kind: FileKind::Module,
    })
    .intern(&db);
    let syntax = db.file_syntax(file).unwrap();

    let syntax = syntax
        .descendants(&db)
        .find(|s| s.kind(&db) == SyntaxKind::ModuleItemList)
        .unwrap();

    let module_item = ModuleItemList::from_syntax_node(&db, syntax)
        .elements(&db)
        .into_iter()
        .next()
        .unwrap();

    (db, module_item)
}

fn parse_expr(
    plugin_suite: PluginSuite,
    code: String,
    cfg_set: Arc<CfgSet>,
) -> (ProcMacroServerDatabase, ExprInlineMacro) {
    let db = ProcMacroServerDatabase::new(plugin_suite, cfg_set);
    let file = FileLongId::Virtual(VirtualFile {
        parent: None,
        name: "parser_input".into(),
        content: code.into(),
        code_mappings: [].into(),
        kind: FileKind::Expr,
    })
    .intern(&db);
    let syntax = db.file_expr_syntax(file).unwrap();

    let syntax = syntax
        .as_syntax_node()
        .descendants(&db)
        .find(|s| s.kind(&db) == SyntaxKind::ExprInlineMacro)
        .unwrap();

    let syntax = ExprInlineMacro::from_syntax_node(&db, syntax);

    (db, syntax)
}
