import { store } from "@frontiers-labs/argon";

/// Number of inbox suggestions still awaiting review. Seeded server-side on
/// first paint, then kept current by the user-events socket and by the Inbox
/// page itself.
export const inbox = store({
  pending: 0,
});
