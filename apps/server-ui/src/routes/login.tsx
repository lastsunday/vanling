import { getVersion } from "@/api";
import {
  Button,
  Container,
  Flex,
  Paper,
  PasswordInput,
  Text,
  TextInput,
  Title
} from '@mantine/core';
import { notifications } from '@mantine/notifications';
import { useQuery } from '@tanstack/react-query';
import { createFileRoute, redirect, useRouter, useRouterState } from '@tanstack/react-router';
import React, { useState } from 'react';
import { z } from 'zod';
import { useAuth } from '../hooks/auth';
import classes from './login.module.css';
import { useTranslation } from "react-i18next";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";

const fallback = '/admin' as const

export const Route = createFileRoute('/login')({
  validateSearch: z.object({
    redirect: z.string().optional().catch(''),
  }),
  beforeLoad: ({ context, search }) => {
    if (context.auth.isAuthenticated) {
      throw redirect({ to: search.redirect || fallback })
    }
  },
  component: RouteComponent,
})

function RouteComponent() {
  const auth = useAuth()
  const router = useRouter()
  const isLoading = useRouterState({ select: (s) => s.isLoading })
  const navigate = Route.useNavigate()
  const [isSubmitting, setIsSubmitting] = useState(false)
  const search = Route.useSearch()

  const { t } = useTranslation();

  const { data: version, isLoading: isVersionLoading, isSuccess: isVersionSuccess } = useQuery({
    queryKey: [],
    queryFn: getVersion
  })

  const onFormSubmit = async (evt: React.FormEvent<HTMLFormElement>) => {
    setIsSubmitting(true)
    try {
      evt.preventDefault()
      const data = new FormData(evt.currentTarget)
      const accountValue = data.get('account')
      const passwordValue = data.get('password')

      if (!accountValue || !passwordValue) return
      const account = accountValue.toString()
      const password = passwordValue.toString()
      await auth.login(account, password);

      await router.invalidate()


      await navigate({ to: search.redirect || fallback })
    } catch (error) {
      notifications.show({
        color: "red",
        title: t('error'),
        message: `${error}`
      })
    } finally {
      setIsSubmitting(false)
    }
  }

  const isLoggingIn = isLoading || isSubmitting

  return (
    <Container size={420} my={40}>
      <Title ta="center" className={classes.title}>
        {t('login.welcome')}
        <Text size='xs'>{t('version')}: {isVersionLoading ? '...' : isVersionSuccess ? version : t('version_unknown')}</Text>
      </Title>

      <Paper withBorder shadow="sm" p={22} mt={30} radius="md">
        <form className="mt-4 max-w-lg" onSubmit={onFormSubmit}>
          <TextInput name="account" label={t('login.form.account')} placeholder={t('login.form.account_hint')} required radius="md" minLength={4} maxLength={16} />
          <PasswordInput name="password" label={t('login.form.password')} placeholder={t('login.form.password_hint')} required mt="md" radius="md" minLength={6} maxLength={16} />
          <Flex justify="end" className="mt-sm">
            <LanguageSwitcher></LanguageSwitcher>
          </Flex>
          <Button type='submit' fullWidth mt="sm" radius="md" disabled={isSubmitting}>
            {isLoggingIn ? `${t('loading')}...` : t('login.title')}
          </Button>
        </form>
      </Paper>
    </Container>
  );
}
