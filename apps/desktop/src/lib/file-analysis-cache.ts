import type { FileAnalysis } from "@diffkin/protocol";

const maxEntries = 16;
const maxSerializedChars = 256 * 1024;

/** WebView-local results for the current comparison or multi-repository workspace. */
export class FileAnalysisCache {
  private entries = new Map<string, FileAnalysis>();

  get(snapshot: string, file: string) {
    const key = JSON.stringify([snapshot, file]);
    const result = this.entries.get(key);
    if (result) {
      this.entries.delete(key);
      this.entries.set(key, result);
    }
    return result;
  }

  remember(snapshot: string, result: FileAnalysis) {
    // Skip large sources before measuring their complete structured result.
    const sourceChars = (result.file.before?.length ?? 0) + (result.file.after?.length ?? 0);
    if (sourceChars > maxSerializedChars || JSON.stringify(result).length > maxSerializedChars)
      return;
    const key = JSON.stringify([snapshot, result.file.summary.id]);
    this.entries.delete(key);
    this.entries.set(key, result);
    if (this.entries.size > maxEntries) this.entries.delete(this.entries.keys().next().value!);
  }

  clear() {
    this.entries.clear();
  }
}
