use anyhow::Result;

use cairo_lang_semantic::plugin::PluginSuite;
use scarb::{
    compiler::{
        plugin::proc_macro::{ProcMacroHost, ProcMacroHostPlugin},
        CairoCompilationUnit, CompilationUnit,
    },
    core::{Config, Workspace},
    ops::{self, FeaturesOpts, FeaturesSelector},
};
use scarb_ui::components::ValueMessage;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tracing::instrument(skip_all, level = "info")]
pub fn run(config: &Config) -> Result<()> {
    let ws = ops::read_workspace(config.manifest_path(), config)?;
    let resolve = ops::resolve_workspace(&ws)?;
    let compilation_units = ops::generate_compilation_units(
        &resolve,
        &FeaturesOpts {
            features: FeaturesSelector::AllFeatures,
            no_default_features: false,
        },
        &ws,
    )?;

    let mut suite = PluginSuite::default();

    for unit in compilation_units {
        match unit {
            CompilationUnit::ProcMacro(_) => {
                ops::compile_unit(unit, &ws)?;
            }
            CompilationUnit::Cairo(unit) => {
                suite.add(ProcMacroHostPlugin::build_plugin_suite(load_plugins(
                    unit, &ws,
                )?));
            }
        }
    }

    config.tokio_handle().block_on(async {
        // Listen on 0 port so OS assign free one.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        config
            .ui()
            .print(ValueMessage::new("server_address", &addr));
        ops::start_proc_macro_server(suite, listener).await
    })
}

fn load_plugins(
    unit: CairoCompilationUnit,
    ws: &Workspace<'_>,
) -> Result<Arc<ProcMacroHostPlugin>> {
    let mut proc_macros = ProcMacroHost::default();

    for plugin_info in unit
        .cairo_plugins
        .into_iter()
        .filter(|plugin_info| !plugin_info.builtin)
    {
        proc_macros.register(plugin_info.package, ws.config())?;
    }

    Ok(Arc::new(proc_macros.into_plugin()?))
}
