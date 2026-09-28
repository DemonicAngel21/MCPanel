import type { PropertySchemaDto } from "@/bindings/PropertySchemaDto";

/** Client-side hint only; the backend validates authoritatively. */
export function validateProperty(schema: PropertySchemaDto | null, value: string): string | null {
  if (!schema) return null;
  if (schema.kind === "integer") {
    if (value.trim() === "") return null;
    const n = Number(value);
    if (!Number.isInteger(n)) return "Must be a whole number";
    if (schema.min != null && n < schema.min) return `Minimum is ${schema.min}`;
    if (schema.max != null && n > schema.max) return `Maximum is ${schema.max}`;
  }
  return null;
}
