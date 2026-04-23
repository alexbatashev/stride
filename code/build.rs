fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/agent.capnp")
        .run()
        .expect("compiling Cap'n'Proto schema");
}
