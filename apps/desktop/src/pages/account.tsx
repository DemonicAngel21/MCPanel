import { PageBody, PageHeader } from "@/app/app-shell";
import { AccountPanel } from "@/components/account-panel";
import { Card } from "@/components/ui/primitives";

export function AccountPage() {
  return (
    <>
      <PageHeader title="Account" description="Your MCPanel account is optional. MCPanel and your servers work without one." />
      <PageBody className="max-w-2xl">
        <Card className="p-5">
          <AccountPanel />
        </Card>
      </PageBody>
    </>
  );
}
