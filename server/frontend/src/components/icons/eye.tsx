import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host { align-items: center; display: inline-flex; height: 16px; width: 16px; }
  svg { height: 100%; width: 100%; }
`;

export function IconEye(): Component {
  return (
    <>
      <style>{styles}</style>
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z" />
        <circle cx="12" cy="12" r="3" />
      </svg>
    </>
  );
}
