import type { OS } from '../components/WindowChrome';

export type CapsuleMessageKind = 'default' | 'processing' | 'error';

export interface CapsulePillMetrics {
  width: number;
  height: number;
  textWidth: number;
  boxSizing: 'border-box' | 'content-box';
}

export interface CapsuleHostMetrics {
  width: number;
  height: number;
  horizontalInset: number;
  bottomInset: number;
  badgeGap: number;
  boxSizing: 'border-box' | 'content-box';
}

export interface CapsuleMessageLayout {
  allowWrap: boolean;
  lineClamp: number;
}

export function getCapsulePillMetrics(os: OS): CapsulePillMetrics {
  if (os === 'win') {
    // Windows metrics 必须与 src-tauri/src/lib.rs 的原生命中框合同保持一致。
    return { width: 196, height: 52, textWidth: 104, boxSizing: 'border-box' };
  }

  return { width: 176, height: 42, textWidth: 84, boxSizing: 'border-box' };
}

// macOS 走 1.2.11 calc 布局，不依赖 host metrics；Windows 端要更大的 host
// 装下阴影 inset，仍用这一份。
export function getCapsuleHostMetrics(
  os: OS,
  translationActive: boolean,
): CapsuleHostMetrics {
  if (os === 'win') {
    const horizontalInset = 12;
    const pill = getCapsulePillMetrics(os);
    return {
      width: pill.width + horizontalInset * 2,
      height: translationActive ? 118 : 84,
      horizontalInset,
      bottomInset: 12,
      badgeGap: 8,
      boxSizing: 'border-box',
    };
  }
  return {
    width: 176,
    height: 42,
    horizontalInset: 0,
    bottomInset: 0,
    badgeGap: 8,
    boxSizing: 'border-box',
  };
}

export function getCapsuleMessageLayout(
  os: OS,
  kind: CapsuleMessageKind,
): CapsuleMessageLayout {
  // エラー文言は長くなりうるので 2 行許可。整形中ラベル（processing）は短い
  // 固定文言なので 1 行固定（折り返し禁止）にして、狭いテキストスロットでも
  // 2 行に割れないようにする。
  if (os === 'win' && kind === 'error') {
    return { allowWrap: true, lineClamp: 2 };
  }

  return { allowWrap: false, lineClamp: 1 };
}
