import { store } from "@frontiers-labs/argon";

// Shared state for the thread page's right side panel (desktop) and the
// equivalent mobile full-screen dialogs. Open/closed, the active tab, and the
// drilled-in subagent all live here so the controller, the panel, and the
// mobile dialogs stay in sync without window events or host props.
export const sidePanel = store({
	open: false,
	tab: "files" as "files" | "subagents",
	selectedSubagent: "",
});
