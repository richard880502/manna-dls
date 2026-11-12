import type { PrepRoute } from "../../api/types";
import { PREP_ROUTES, PREP_ROUTE_DESCRIPTIONS, PREP_ROUTE_LABELS } from "../../lib/preferences";

interface RouteOptionsProps {
  name: string;
  legend: string;
  value: PrepRoute;
  onChange: (route: PrepRoute) => void;
  disabled?: boolean;
  hint?: string;
}

/** The four prep routes (備課菜單) as a radio group. Only rendered when route choice is enabled. */
export function RouteOptions({ name, legend, value, onChange, disabled = false, hint }: RouteOptionsProps) {
  return (
    <fieldset className="route-options" disabled={disabled}>
      <legend className="field-label">{legend}</legend>
      <div className="route-option-grid">
        {PREP_ROUTES.map((route) => (
          <label key={route} className={`route-option${route === value ? " is-selected" : ""}`}>
            <input
              type="radio"
              name={name}
              value={route}
              checked={route === value}
              onChange={() => onChange(route)}
            />
            <strong>{PREP_ROUTE_LABELS[route]}</strong>
            <small>{PREP_ROUTE_DESCRIPTIONS[route]}</small>
          </label>
        ))}
      </div>
      {hint && <p className="field-hint">{hint}</p>}
    </fieldset>
  );
}
