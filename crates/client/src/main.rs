//! Native client entry. Used for local profiling builds; the shipped game is wasm.

fn main() {
    client::build_app().run();
}
