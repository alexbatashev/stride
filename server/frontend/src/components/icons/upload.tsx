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

export function IconUpload(): Component {
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
        <path d="M12 3v12" />
        <path d="m17 8-5-5-5 5" />
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      </svg>
    </>
  );
}
