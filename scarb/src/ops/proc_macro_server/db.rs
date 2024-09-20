use cairo_lang_defs::db::{ext_as_virtual_impl, DefsDatabase, DefsGroup};
use cairo_lang_filesystem::{
    cfg::CfgSet,
    db::{init_files_group, ExternalFiles, FilesDatabase, FilesGroup},
    ids::VirtualFile,
};

use cairo_lang_parser::db::{ParserDatabase, ParserGroup};
use cairo_lang_semantic::plugin::PluginSuite;
use cairo_lang_syntax::node::db::{SyntaxDatabase, SyntaxGroup};
use cairo_lang_utils::Upcast;
use std::sync::Arc;

#[salsa::database(DefsDatabase, FilesDatabase, ParserDatabase, SyntaxDatabase)]
pub struct ProcMacroServerDatabase {
    storage: salsa::Storage<Self>,
}

impl salsa::Database for ProcMacroServerDatabase {}

impl ExternalFiles for ProcMacroServerDatabase {
    fn ext_as_virtual(&self, external_id: salsa::InternId) -> VirtualFile {
        ext_as_virtual_impl(self.upcast(), external_id)
    }
}

impl Upcast<dyn FilesGroup> for ProcMacroServerDatabase {
    fn upcast(&self) -> &(dyn FilesGroup + 'static) {
        self
    }
}

impl Upcast<dyn SyntaxGroup> for ProcMacroServerDatabase {
    fn upcast(&self) -> &(dyn SyntaxGroup + 'static) {
        self
    }
}

impl Upcast<dyn DefsGroup> for ProcMacroServerDatabase {
    fn upcast(&self) -> &(dyn DefsGroup + 'static) {
        self
    }
}

impl Upcast<dyn ParserGroup> for ProcMacroServerDatabase {
    fn upcast(&self) -> &(dyn ParserGroup + 'static) {
        self
    }
}

impl ProcMacroServerDatabase {
    /// Creates a new instance of the database.
    pub fn new(suite: PluginSuite, cfg_set: Arc<CfgSet>) -> Self {
        let mut db = Self {
            storage: Default::default(),
        };

        init_files_group(&mut db);

        db.set_cfg_set(cfg_set);

        let mut plugin_suite = PluginSuite::default();

        plugin_suite.add(suite);

        db.apply_plugin_suite(plugin_suite);

        db
    }

    fn apply_plugin_suite(&mut self, plugin_suite: PluginSuite) {
        self.set_macro_plugins(plugin_suite.plugins);
        self.set_inline_macro_plugins(plugin_suite.inline_macro_plugins.into());
    }
}
