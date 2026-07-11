import { store } from "@frontiers-labs/argon";

export const sidebar = store({
  status: "open",
  activeThread: "",
  activeProject: "",
  activePage: "",
  runningThreads: [] as string[],
});
