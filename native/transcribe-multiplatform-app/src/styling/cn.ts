// Runs with bun: shadcn's class merger.
import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export const cn = (...inputs: ClassValue[]): string => twMerge(clsx(inputs));

/** Base UI's `ref` types clash with Preact's under compat; these components never forward a ref. */
export type PrimitiveProps<Props> = Omit<Props, "ref">;
