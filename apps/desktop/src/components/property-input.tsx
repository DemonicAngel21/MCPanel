import type { PropertySchemaDto } from "@/bindings/PropertySchemaDto";
import { Select } from "./ui/overlays";
import { Input, Switch, Textarea } from "./ui/primitives";

/** Control for one server.properties value, driven by the version-aware schema. */
export function PropertyInput({
  schema,
  value,
  onChange,
  id,
  placeholder,
}: {
  schema: PropertySchemaDto | null;
  value: string;
  onChange: (v: string) => void;
  id?: string;
  placeholder?: string;
}) {
  if (!schema) return <Input id={id} value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} />;
  switch (schema.kind) {
    case "boolean":
      return <Switch id={id} checked={value === "true"} onCheckedChange={(c) => onChange(c ? "true" : "false")} />;
    case "enum":
      return (
        <Select
          id={id}
          value={value || undefined}
          onValueChange={onChange}
          placeholder={schema.default ?? "Select"}
          options={schema.values.map((v) => ({ value: v, label: v.charAt(0).toUpperCase() + v.slice(1) }))}
        />
      );
    case "integer":
      return (
        <Input
          id={id}
          inputMode="numeric"
          value={value}
          placeholder={schema.default ?? undefined}
          onChange={(e) => onChange(e.target.value.replace(/[^0-9-]/g, ""))}
        />
      );
    case "text":
      return <Textarea id={id} rows={2} value={value} placeholder={schema.default ?? placeholder} onChange={(e) => onChange(e.target.value)} />;
    default:
      return (
        <>
          <Input
            id={id}
            value={value}
            placeholder={schema.default || placeholder}
            onChange={(e) => onChange(e.target.value)}
            list={schema.suggestions.length > 0 ? `${id}-suggestions` : undefined}
          />
          {schema.suggestions.length > 0 && (
            <datalist id={`${id}-suggestions`}>
              {schema.suggestions.map((s) => (
                <option key={s} value={s} />
              ))}
            </datalist>
          )}
        </>
      );
  }
}
