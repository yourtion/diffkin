import type { EditorView } from "@codemirror/view";
import type { ChangeEvent } from "@diffkin/protocol";

const svgNamespace = "http://www.w3.org/2000/svg";
export function bindMoveConnection(
  stage: HTMLElement,
  overlay: SVGSVGElement,
  left: EditorView,
  right: EditorView,
  event: ChangeEvent,
  beforeOffset: number,
  afterOffset: number,
): () => void {
  let frame = 0;
  let closed = false;
  const path = document.createElementNS(svgNamespace, "path");
  const start = document.createElementNS(svgNamespace, "circle");
  const end = document.createElementNS(svgNamespace, "circle");
  const title = document.createElementNS(svgNamespace, "title");
  path.setAttribute("class", "move-path");
  start.setAttribute("r", "3");
  end.setAttribute("r", "3");
  start.setAttribute("class", "move-endpoint");
  end.setAttribute("class", "move-endpoint");
  overlay.replaceChildren(title, path, start, end);
  overlay.setAttribute("role", "img");
  overlay.setAttribute(
    "aria-label",
    `代码移动：${event.title}，修改前第 ${event.before?.startLine} 行 → 修改后第 ${event.after?.startLine} 行`,
  );
  function draw() {
    frame = 0;
    if (closed || !stage.isConnected) return;
    const bounds = stage.getBoundingClientRect();
    if (bounds.width < 1 || bounds.height < 1) return;
    const point = (view: EditorView, offset: number) => {
      const pos = Math.max(0, Math.min(offset, view.state.doc.length));
      const coordinates = view.coordsAtPos(pos);
      const block = view.lineBlockAt(pos);
      const y =
        (coordinates
          ? (coordinates.top + coordinates.bottom) / 2
          : view.documentTop + block.top + Math.min(block.height, view.defaultLineHeight) / 2) -
        bounds.top;
      return {
        y: Math.max(12, Math.min(bounds.height - 12, y)),
        clipped: y < 12 ? "上方" : y > bounds.height - 12 ? "下方" : "",
      };
    };
    const a = point(left, beforeOffset);
    const b = point(right, afterOffset);
    const x1 = Math.max(4, left.dom.getBoundingClientRect().right - bounds.left + 5);
    const x2 = Math.min(bounds.width - 4, right.dom.getBoundingClientRect().left - bounds.left - 5);
    const center = (x1 + x2) / 2;
    overlay.setAttribute("viewBox", `0 0 ${bounds.width} ${bounds.height}`);
    path.setAttribute("d", `M ${x1} ${a.y} C ${center} ${a.y}, ${center} ${b.y}, ${x2} ${b.y}`);
    path.setAttribute("stroke-dasharray", a.clipped || b.clipped ? "4 3" : "none");
    start.setAttribute("cx", String(x1));
    start.setAttribute("cy", String(a.y));
    end.setAttribute("cx", String(x2));
    end.setAttribute("cy", String(b.y));
    title.textContent = `${event.title}：L${event.before?.startLine} → L${event.after?.startLine}${a.clipped ? `；起点在视口${a.clipped}` : ""}${b.clipped ? `；终点在视口${b.clipped}` : ""}。使用起点/终点按钮定位；虚线表示端点位于视口外。`;
    overlay.dataset.beforeClipped = a.clipped;
    overlay.dataset.afterClipped = b.clipped;
  }
  const schedule = () => {
    if (!closed && !frame) frame = requestAnimationFrame(draw);
  };
  // Capture scroll events from the outer viewport and either editor's horizontal scroller.
  stage.addEventListener("scroll", schedule, true);
  window.addEventListener("resize", schedule);
  const resize = new ResizeObserver(schedule);
  [stage, left.dom, right.dom].forEach((element) => resize.observe(element));
  const mutation = new MutationObserver(schedule);
  mutation.observe(left.contentDOM, { subtree: true, childList: true });
  mutation.observe(right.contentDOM, { subtree: true, childList: true });
  schedule();
  return () => {
    closed = true;
    cancelAnimationFrame(frame);
    resize.disconnect();
    mutation.disconnect();
    stage.removeEventListener("scroll", schedule, true);
    window.removeEventListener("resize", schedule);
    overlay.replaceChildren();
    overlay.removeAttribute("aria-label");
  };
}
