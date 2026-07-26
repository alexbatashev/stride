import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    align-items: center;
    display: inline-flex;
    height: 20px;
    justify-content: center;
    width: 20px;
  }
  svg {
    height: 100%;
    width: 100%;
  }
`;

export function IconStrideMark(): Component {
  return (
    <>
      <style>{styles}</style>
      <svg
        xmlns="http://www.w3.org/2000/svg"
        viewBox="0 0 24 24"
        fill="currentColor"
        aria-hidden="true"
      >
        <path d="M7.65 4H21l-3.42 4.34H9.43L7.86 10.3l3.05 2.06h6.47c2.3 0 3.62 2.62 2.2 4.43L17.06 20H3l3.42-4.34h8.71l1.25-1.58-3.05-2.06H6.72c-2.26 0-3.58-2.54-2.24-4.36L7.65 4Z" />
      </svg>
    </>
  );
}
