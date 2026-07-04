import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: inline-flex;
    align-items: center;
    width: 24px;
    height: 24px;
  }
  svg {
    width: 100%;
    height: 100%;
  }
`;

export function IconArchive(): Component {
  return (
    <>
      <style>{styles}</style>
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="24"
        height="24"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <rect width="20" height="5" x="2" y="3" rx="1" />
        <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
        <path d="M10 12h4" />
      </svg>
    </>
  );
}
