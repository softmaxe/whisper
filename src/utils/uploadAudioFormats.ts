import { UPLOAD_AUDIO_EXTENSIONS } from "../constants/uploadAudioFormats.json";

export function isSupportedUploadFile(fileName: string): boolean {
  const dot = fileName.lastIndexOf(".");
  return dot > 0 && UPLOAD_AUDIO_EXTENSIONS.includes(fileName.slice(dot + 1).toLowerCase());
}
