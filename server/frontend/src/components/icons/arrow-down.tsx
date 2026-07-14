import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    align-items: center;
    display: inline-flex;
    height: 24px;
    width: 24px;
  }
  svg {
    height: 100%;
    width: 100%;
  }
`;

export function IconArrowDown(): Component {
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
        <path d="M12 5v14" />
        <path d="m19 12-7 7-7-7" />
      </svg>
    </>
  );
}
