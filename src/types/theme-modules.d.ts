declare module "material-dynamic-colors" {
  type DynamicColorScheme = {
    light: Record<string, string>;
    dark: Record<string, string>;
  };

  export default function materialDynamicColors(
    hexCode: string,
  ): Promise<DynamicColorScheme>;
}

declare module "@material/material-color-utilities" {
  export function hexFromArgb(argb: number): string;
  export function argbFromHex(hex: string): number;
  export function customColor(
    sourceColor: number,
    color: {
      value: number;
      blend: boolean;
      name: string;
    },
  ): { value: number };

  export class Hct {
    hue: number;
    chroma: number;
    static fromInt(argb: number): Hct;
  }

  export class TonalPalette {
    static fromHueAndChroma(hue: number, chroma: number): TonalPalette;
    tone(tone: number): number;
  }
}
