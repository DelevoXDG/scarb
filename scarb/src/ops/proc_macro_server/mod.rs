use anyhow::Result;
use cairo_lang_defs::db::DefsGroup;
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
use conversions::{plugin_diagnostic, plugin_generated_file, MetadataDataHolder};
use db::ProcMacroServerDatabase;
use proc_macro_server_api::{
    proc_macros_server::{ProcMacros, ProcMacrosServer},
    reflection_service,
    tonic::{self, Request, Response, Status},
    DefinedAttributesResponse, DefinedInlineMacrosResponse, Empty, ExpandInlineParams,
    ExpandInlineResponse, ExpandParams, ExpandResponse,
};
use std::sync::Arc;
use tokio::net::TcpListener;

mod conversions;
mod db;

pub async fn start_proc_macro_server(suite: PluginSuite, listener: TcpListener) -> Result<()> {
    let reflection_service = reflection_service();

    tonic::transport::Server::builder()
        .add_service(ProcMacrosServer::new(ProcMacrosService { suite }))
        .add_service(reflection_service)
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
        .await?;

    Ok(())
}

struct ProcMacrosService {
    suite: PluginSuite,
}

#[tonic::async_trait]
impl ProcMacros for ProcMacrosService {
    async fn expand(&self, req: Request<ExpandParams>) -> Result<Response<ExpandResponse>, Status> {
        let req = req.into_inner();
        let metadata = MetadataDataHolder::new(req.metadata);

        let (db, module_item) = self.parse(req.code.to_owned(), metadata.cfg_set());

        let plugins = db.macro_plugins();
        let result = plugins.first().unwrap().generate_code(
            db.upcast(),
            module_item,
            &metadata.macro_plugin_metadata(),
        );

        let code = result.code.map(plugin_generated_file);
        let diagnostics = result
            .diagnostics
            .into_iter()
            .map(|d| plugin_diagnostic(d, &db))
            .collect();

        Ok(Response::new(ExpandResponse {
            code,
            diagnostics,
            remove_original_item: result.remove_original_item,
        }))
    }

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
            macros: ProcMacroServerDatabase::new(self.suite.clone(), Default::default())
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
            attributes: ProcMacroServerDatabase::new(self.suite.clone(), Default::default())
                .macro_plugins()
                .into_iter()
                .flat_map(|plugin| plugin.declared_attributes())
                .collect(),
        }))
    }
}

impl ProcMacrosService {
    fn parse(&self, code: String, cfg_set: Arc<CfgSet>) -> (ProcMacroServerDatabase, ModuleItem) {
        let db = ProcMacroServerDatabase::new(self.suite.clone(), cfg_set);
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
        &self,
        code: String,
        cfg_set: Arc<CfgSet>,
    ) -> (ProcMacroServerDatabase, ExprInlineMacro) {
        let db = ProcMacroServerDatabase::new(self.suite.clone(), cfg_set);
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
}
