import { Component } from "@frontiers-labs/argon";
import { browserStyles } from "./app-files-styles.js";

export function AppFileBrowser(): Component {
  return <><style>{browserStyles}</style><header><h1>Files</h1></header><app-file-explorer></app-file-explorer></>;
}
