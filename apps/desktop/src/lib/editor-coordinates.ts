/** Map original UTF-16 offsets to CodeMirror's LF-normalized document without rescanning prefixes. */
export function editorCoordinates(source: string) {
  const removedAt: number[] = [];
  for (
    let offset = source.indexOf("\r\n");
    offset >= 0;
    offset = source.indexOf("\r\n", offset + 2)
  )
    removedAt.push(offset + 2);
  return (offset: number) => {
    let low = 0;
    let high = removedAt.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (removedAt[middle] <= offset) low = middle + 1;
      else high = middle;
    }
    return offset - low;
  };
}
