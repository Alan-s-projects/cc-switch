import type { SettingsFormState } from "@/hooks/useSettings";
import { ProxyPanel } from "@/components/proxy/ProxyPanel";
import { GlobalProxySettings } from "./GlobalProxySettings";
import {
  Accordion,
  AccordionItem,
  AccordionTrigger,
  AccordionContent,
} from "@/components/ui/accordion";

export function ProxyTabContent(_props: {
  settings: SettingsFormState;
  onAutoSave: (updates: Partial<SettingsFormState>) => Promise<boolean>;
}) {
  return (
    <div className="space-y-5">
      <ProxyPanel />
      <Accordion type="multiple" className="space-y-3">
        <AccordionItem value="network">
          <AccordionTrigger>Outbound network</AccordionTrigger>
          <AccordionContent>
            <GlobalProxySettings />
          </AccordionContent>
        </AccordionItem>
      </Accordion>
    </div>
  );
}
