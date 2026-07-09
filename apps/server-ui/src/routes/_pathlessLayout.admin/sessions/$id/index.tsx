import { SessionDetail } from '@/components/SessionDetail';
import {
  Anchor,
  Group,
  Title,
} from '@mantine/core';
import { createFileRoute, useParams, useRouter } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute(
  '/_pathlessLayout/admin/sessions/$id/',
)({
  component: RouteComponent,
});

function RouteComponent() {
  const { t } = useTranslation();
  const router = useRouter();
  const { id } = useParams({ from: Route.id });

  return (
    <>
      <Group mb="lg">
        <Anchor component="button" onClick={() => router.history.back()}>
          {t('sessions.detail.back')}
        </Anchor>
        <Title>{t('sessions.detail.title')}</Title>
      </Group>
      <SessionDetail sessionId={id} />
    </>
  );
}
