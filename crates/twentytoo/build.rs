//! Compiles the built-in templates into the binary.
//!
//! `embed_templates!` also validates template syntax at build time — an
//! invalid built-in template fails the build, not the first request
//! (`05-template-engine.md` §5.3).

fn main() {
    minijinja_embed::embed_templates!("templates", &[".j2"]);
}
