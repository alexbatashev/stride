import { store } from "@frontiers-labs/argon";

export const settings = store({
  open: false,
  activeSection: "personal",
  notice: "",
  username: "",
  fullName: "",
});
