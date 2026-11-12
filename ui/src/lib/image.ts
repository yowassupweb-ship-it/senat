/**
 * A picture cut to a square from its centre and scaled down, as a PNG: what an avatar needs. A photo from a phone is
 * several megabytes and never square; this keeps what is stored small and the circle full.
 */
export async function squarePng(file: Blob, size = 256): Promise<Blob> {
  let bmp: ImageBitmap;
  try {
    bmp = await createImageBitmap(file);
  } catch {
    throw new Error('Не удалось прочитать картинку. Подойдёт png, jpg, gif или webp.');
  }
  const side = Math.min(bmp.width, bmp.height);
  const cv = document.createElement('canvas');
  cv.width = cv.height = size;
  const ctx = cv.getContext('2d');
  if (!ctx) throw new Error('Не удалось обработать картинку');
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(bmp, (bmp.width - side) / 2, (bmp.height - side) / 2, side, side, 0, 0, size, size);
  bmp.close?.();
  return new Promise((resolve, reject) => cv.toBlob((b) => (b ? resolve(b) : reject(new Error('Не удалось обработать картинку'))), 'image/png'));
}
