export interface FrontMatterErrorDetailSyntax {
  type: 'syntax';
  line?: number;
  column?: number;
  message?: string;
}

export interface FrontMatterErrorDetailValidation {
  type: 'validation';
  property_path?: string;
  message?: string;
}

export type FrontMatterErrorDetail =
  | FrontMatterErrorDetailSyntax
  | FrontMatterErrorDetailValidation;

function isRootPropertyPath(propertyPath: string | undefined): boolean {
  return propertyPath === '$';
}

function formatFrontMatterTarget(propertyPath: string | undefined): string | null {
  if (!propertyPath || propertyPath.length === 0 || isRootPropertyPath(propertyPath)) {
    return null;
  }
  return propertyPath;
}

function translateFrontMatterValidationMessage(
  message: string,
  propertyPath: string | undefined,
): string | null {
  if (message === 'front matter top-level must be object') {
    return 'front matter のトップレベルは object である必要があります';
  }

  const mustBeObject = message.match(/^(.+) must be object$/);
  if (mustBeObject) {
    return `front matter の ${mustBeObject[1]} は object である必要があります`;
  }

  const mustNotBeEmpty = message.match(/^(.+) must not be empty$/);
  if (mustNotBeEmpty) {
    return `front matter の ${mustNotBeEmpty[1]} は空にできません`;
  }

  if (message === 'tag must not contain whitespace or control characters') {
    return 'front matter のタグには空白文字や制御文字を含められません';
  }

  if (message === 'mcp.name is required for prompt primitive') {
    return 'front matter の mcp.primitive が prompt の場合、mcp.name は必須です';
  }

  if (message === 'mcp.arguments is not allowed for resource primitive') {
    return 'front matter の mcp.primitive が resource の場合、mcp.arguments は指定できません';
  }

  if (message === 'unsupported mcp primitive') {
    return 'front matter の mcp.primitive は未対応の値です';
  }

  if (message.length === 0) {
    return null;
  }

  const target = formatFrontMatterTarget(propertyPath);
  if (target) {
    return `front matter の項目が不正です: ${message} (対象: ${target})`;
  }
  return `front matter の項目が不正です: ${message}`;
}

export function resolveFrontMatterErrorMessage(
  detail: FrontMatterErrorDetail | undefined,
): string {
  if (!detail) {
    return 'front matter の記述が不正です';
  }

  if (detail.type === 'syntax') {
    if (typeof detail.line === 'number' && typeof detail.column === 'number') {
      return `front matter の構文エラーです: ${detail.line}行目 ${detail.column}列目を確認してください`;
    }
    if (typeof detail.line === 'number') {
      return `front matter の構文エラーです: ${detail.line}行目を確認してください`;
    }
    return 'front matter の構文エラーです';
  }

  if (typeof detail.message === 'string') {
    const translated = translateFrontMatterValidationMessage(
      detail.message,
      detail.property_path,
    );
    if (translated) {
      return translated;
    }
  }

  if (typeof detail.property_path === 'string' && detail.property_path.length > 0) {
    const target = formatFrontMatterTarget(detail.property_path);
    if (target) {
      return `front matter の項目が不正です: ${target} を確認してください`;
    }
    return 'front matter のトップレベル構造が不正です';
  }

  return 'front matter の項目が不正です';
}
