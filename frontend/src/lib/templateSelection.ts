import type { TemplatePageItem } from '../api/pages';

export function sortTemplateItems(items: TemplatePageItem[]): TemplatePageItem[] {
  return [...items].sort((left, right) =>
    left.name.localeCompare(right.name, undefined, {
      numeric: true,
      sensitivity: 'base',
    }) || left.page_id.localeCompare(right.page_id),
  );
}

export function resolveSelectedTemplateId(
  currentId: string,
  items: TemplatePageItem[],
): string {
  if (currentId.length > 0 && items.some((item) => item.page_id === currentId)) {
    return currentId;
  }

  const sorted = sortTemplateItems(items);
  return sorted.length > 0 ? sorted[0].page_id : '';
}
