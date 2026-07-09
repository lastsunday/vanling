import {
  Badge,
  Container,
  Group,
  Title
} from '@mantine/core';
import { createFileRoute } from '@tanstack/react-router';
import { WebsiteFooter } from '../../components/website/WebsiteFooter';
import { WebsiteHeader } from '../../components/website/WebsiteHeader';
import classes from './index.module.css';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/_pathlessLayout/')({
  component: HomeComponent,
})

function HomeComponent() {

  const { t } = useTranslation();

  return <Container py="md">
    <WebsiteHeader />
    <Container size="lg" py="xl">
      <Group justify="center">
        <Badge color='pink' variant="filled" size="lg">
          {t('core')}
        </Badge>
      </Group>

      <Title order={2} className={classes.title} ta="center" mt="sm">
        {t('title')}
      </Title>

    </Container>
    <WebsiteFooter />

  </Container>
}
