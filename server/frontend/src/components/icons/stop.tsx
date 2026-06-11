import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: inline-flex;
    align-items: center;
    width: 16px;
    height: 16px;
  }
  svg {
    width: 100%;
    height: 100%;
  }
`;

export function IconStop(): Component {
  return (
    <>
      <style>{styles}</style>
      <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden="true"
      >
        <rect x="4" y="4" width="16" height="16" rx="2" />
      </svg>
    </>
  );
}
