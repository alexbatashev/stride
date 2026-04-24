@0xd9a5a3b5c7e1f2a3;

struct ThreadSummary {
  id        @0 :Text;
  cwd       @1 :Text;
  updatedAt @2 :Int64;
  preview   @3 :Text;
}

struct HistoryMessage {
  seq        @0 :UInt32;
  role       @1 :Text;
  content    @2 :Text;
  thinking   @3 :Text;
  toolCallId @4 :Text;
  createdAt  @5 :Int64;
  toolName   @6 :Text;
}

struct CommandResult {
  shouldExit @0 :Bool;
  threadId   @1 :Text;
}

interface AgentDaemon @0x8e3f1a2b4c5d6e7f {
  startSession       @0 (sink :EventSink, cwd :Text) -> (session :AgentSession, threadId :Text);
  resumeSession      @1 (sink :EventSink, threadId :Text) -> (session :AgentSession);
  resumeLatestForCwd @2 (sink :EventSink, cwd :Text) -> (session :AgentSession, threadId :Text);
  listThreads        @3 (cwd :Text, limit :UInt32) -> (threads :List(ThreadSummary));
  getThreadHistory   @4 (threadId :Text) -> (messages :List(HistoryMessage));
}

interface AgentSession @0xa1b2c3d4e5f60718 {
  sendMessage @0 (text :Text) -> ();
  sendCommand @1 (command :Text) -> (result :CommandResult);
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
