/** Moves focus to a transcript segment rendered with id `segment-<id>`. */
export function showSegment(segmentId: string) {
  const element = document.getElementById(`segment-${segmentId}`);
  if (!element) return;
  element.scrollIntoView?.({ block: "center" });
  element.focus();
}
