/* tslint:disable */
/* eslint-disable */

/* auto-generated by NAPI-RS */

export interface Config {
  ignoreMap: Record<string, string>
  needCollect: boolean
}
export interface Res {
  code: string
  props: Array<string>
}
export function transform(source: string, inputConfig?: Config | undefined | null): Res
