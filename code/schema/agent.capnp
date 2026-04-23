@0xd9a5a3b5c7e1f2a3;

interface AgentDaemon @0x8e3f1a2b4c5d6e7f {
  connect @0 (sink :EventSink) -> (session :AgentSession);
}

interface AgentSession @0xa1b2c3d4e5f60718 {
  sendMessage @0 (text :Text) -> ();
  sendCommand @1 (command :Text) -> ();
  confirm     @2 (answer :Bool) -> ();
  disconnect  @3 () -> ();
}

interface EventSink @0xf1e2d3c4b5a69788 {
  onTextChunk            @0 (text :Text) -> ();
  onThinking             @1 (text :Text) -> ();
  onToolCall             @2 (name :Text) -> ();
  onConfirmationRequired @3 (prompt :Text) -> ();
  onError                @4 (message :Text) -> ();
  onDone                 @5 () -> ();
}
