import { store } from "@frontiers-labs/argon";
import type { ModelOption } from "../shared/model-option.js";
import type { ChatTurn } from "../shared/timeline.js";

export const threadView = store({
  active: false,
  turns: [] as ChatTurn[],
  running: false,
  placeholder: "",
  models: [] as ModelOption[],
  selectedModel: "",
  selectedModelLabel: "Choose model",
  selectedModelReasoningEffort: "",
  approvalMessage: "",
  quizQuestion: "",
  quizOptions: [] as string[],
  quizSubmitting: false,
  error: "",
});
