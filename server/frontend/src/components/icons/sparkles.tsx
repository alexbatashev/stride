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

export function IconSparkles(): Component {
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
        <path d="M9.9 4.2a.6.6 0 0 1 1.1 0l1.4 3.4a.6.6 0 0 0 .3.3l3.4 1.4a.6.6 0 0 1 0 1.1l-3.4 1.4a.6.6 0 0 0-.3.3l-1.4 3.4a.6.6 0 0 1-1.1 0l-1.4-3.4a.6.6 0 0 0-.3-.3l-3.4-1.4a.6.6 0 0 1 0-1.1l3.4-1.4a.6.6 0 0 0 .3-.3z" />
        <path d="M18 3v4" />
        <path d="M20 5h-4" />
        <path d="M17 16v4" />
        <path d="M19 18h-4" />
      </svg>
    </>
  );
}
