/** 全機能で共用する2項目のpacket。 */
export interface WebSocketPacket {
  type: string;
  payload: Record<string, unknown>;
}

/** 機能の意味を解釈せず共通形式だけを検証する。 */
export function parseWebSocketPacket(text: string): WebSocketPacket | null {
  let value: unknown;
  try { value = JSON.parse(text); } catch { return null; }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null;
  const packet = value as Record<string, unknown>;
  if (Object.keys(packet).length !== 2 || typeof packet.type !== 'string' || !packet.type
    || typeof packet.payload !== 'object' || packet.payload === null || Array.isArray(packet.payload)) return null;
  return { type: packet.type, payload: packet.payload as Record<string, unknown> };
}

/** 機能ごとに接続先を変更しない共通URL。 */
export function webSocketUrl(): string {
  return `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/ws`;
}
