import { store } from "@frontiers-labs/argon";
import type { ModelOption } from "../shared/model-option.js";
import type { TimelineMessage } from "../shared/timeline.js";

export const threadView = store({
  active: false,
  messages: [] as TimelineMessage[],
  running: false,
  placeholder: "",
  models: [] as ModelOption[],
  selectedModel: "",
  approvalMessage: "",
  quizQuestion: "",
  quizOptions: [] as string[],
  error: "",
});
