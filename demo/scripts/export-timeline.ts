/**
 * Serialises the Film timeline to JSON for the Python audio synthesiser.
 * Usage: node scripts/export-timeline.ts [out.json]
 */
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { FILM } from "../timeline/index.ts";
import { PATHS } from "./paths.ts";

export function exportTimeline(outFile: string = PATHS.timelineJson): string {
  fs.mkdirSync(path.dirname(outFile), { recursive: true });
  fs.writeFileSync(outFile, JSON.stringify(FILM, null, 2) + "\n");
  return outFile;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.log(`wrote ${exportTimeline(process.argv[2])}`);
}
