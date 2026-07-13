import { Component, onMount } from "@frontiers-labs/argon";
import { settings } from "../stores/settings.js";
import { AppDialog } from "./app-dialog.js";

const SECTIONS = new Set([
  "connections",
  "email",
  "mcp",
  "files",
  "memories",
  "skills",
  "models",
  "threads",
]);

function applySettingsUrl(): void {
  const url = new URL(window.location.href);
  if (url.searchParams.get("settings") !== "open") return;

  const section = url.searchParams.get("section");
  if (section && SECTIONS.has(section)) settings.activeSection = section;

  if (url.searchParams.get("github") === "error") {
    settings.notice = "GitHub sign in failed. Try again.";
  } else if (url.searchParams.get("google") === "error") {
    settings.notice = "Google sign in failed. Try again.";
  } else if (url.searchParams.get("github") === "connected") {
    settings.notice = "GitHub connected.";
  } else if (url.searchParams.get("google") === "connected") {
    settings.notice = "Google connected.";
  }

  settings.open = true;
  for (const key of ["settings", "section", "github", "google"]) {
    url.searchParams.delete(key);
  }
  window.history.replaceState(null, "", `${url.pathname}${url.search}${url.hash}`);
}

export function AppSettingsDialog(): Component {
  onMount(() => applySettingsUrl());

  return (
    <AppDialog
      open={settings.open}
      title="Settings"
      description="Manage how S.T.R.I.D.E. connects, remembers, and works."
      size="settings"
      dialogId="settings"
      on:close={() => {
        settings.open = false;
        settings.notice = "";
      }}
    >
      {settings.open ? <app-settings></app-settings> : ""}
    </AppDialog>
  );
}
