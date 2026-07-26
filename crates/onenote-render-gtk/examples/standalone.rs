use anyhow::{bail, Context, Result};
use gtk::glib;
use gtk::prelude::*;
use onenote_core::OneNoteLoader;
use onenote_render::SceneBuilder;
use onenote_render_gtk::PageView;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

fn main() -> Result<()> {
    let (source, smoke) = arguments()?;
    let loaded = OneNoteLoader::default()
        .load(&source)
        .with_context(|| format!("failed to load {}", source.display()))?;
    let page = loaded
        .notebook
        .pages()
        .max_by_key(|page| page.objects.len())
        .context("the source does not contain a page")?;
    let title = format!("{} - {}", page.title, loaded.notebook.name);
    let scene = Arc::new(
        SceneBuilder::default()
            .build(page, &AtomicBool::new(false))
            .context("failed to build the page scene")?,
    );
    let resources = Arc::new(loaded.resources);

    let application = gtk::Application::builder()
        .application_id("io.github.emsi.OneNoteRendererExample")
        .build();
    application.connect_activate(move |application| {
        let view = PageView::new();
        view.set_resources(Some(Arc::clone(&resources)));
        view.set_scene(Some(Arc::clone(&scene)));

        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title(&title)
            .default_width(1_280)
            .default_height(800)
            .child(view.widget())
            .build();
        window.present();

        if smoke {
            let application = application.clone();
            glib::timeout_add_local_once(Duration::from_millis(750), move || {
                application.quit();
            });
        }
    });
    let status = application.run_with_args::<&str>(&[]);
    if status == glib::ExitCode::SUCCESS {
        Ok(())
    } else {
        bail!("GTK application exited with status {status:?}")
    }
}

fn arguments() -> Result<(PathBuf, bool)> {
    let mut source = None;
    let mut smoke = false;
    for argument in std::env::args_os().skip(1) {
        if argument == "--smoke" {
            smoke = true;
        } else if source.replace(PathBuf::from(argument)).is_some() {
            bail!("usage: standalone <section.one|notebook.onetoc2> [--smoke]");
        }
    }
    let source = source.context("usage: standalone <section.one|notebook.onetoc2> [--smoke]")?;
    Ok((source, smoke))
}
