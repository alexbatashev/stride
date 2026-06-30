type HeightMessage = {
  type: "stride-widget-height";
  height: number;
  href: string;
};

let frame = 0;

function measureHeight(): number {
  const body = document.body;
  const root = document.documentElement;
  return Math.ceil(
    Math.max(
      body?.scrollHeight ?? 0,
      body?.offsetHeight ?? 0,
      root.scrollHeight,
      root.offsetHeight,
      root.clientHeight,
    ),
  );
}

function postHeight(): void {
  frame = 0;
  const message: HeightMessage = {
    type: "stride-widget-height",
    height: measureHeight(),
    href: window.location.href,
  };
  window.parent?.postMessage(message, "*");
}

function schedulePostHeight(): void {
  if (frame !== 0) {
    return;
  }
  frame = window.requestAnimationFrame(postHeight);
}

window.addEventListener("load", schedulePostHeight);
window.addEventListener("resize", schedulePostHeight);

new ResizeObserver(schedulePostHeight).observe(document.documentElement);
if (document.body) {
  new ResizeObserver(schedulePostHeight).observe(document.body);
}

schedulePostHeight();
