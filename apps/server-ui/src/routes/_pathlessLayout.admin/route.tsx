import { getUser, resetPassword } from '@/api';
import { UserResult } from '@/data/user-result';
import { AppShell, AppShellHeader, AppShellNavbar, AppShellMain, Burger, Button, Group, Modal, PasswordInput, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { notifications } from '@mantine/notifications';
import { createFileRoute, Outlet, redirect, useRouter, useRouterState } from '@tanstack/react-router';
import { useEffect, useState } from 'react';
import logo from '../../assets/logo.svg';
import { useAuth } from '../../hooks/auth';
import { UserButton } from '../../widget/UserButton/UserButton';
import classes from './route.module.css';
import { useTranslation } from 'react-i18next';
import { LanguageSwitcher } from '@/components/LanguageSwitcher';

export const Route = createFileRoute('/_pathlessLayout/admin')({
  beforeLoad: ({ context, location }) => {
    if (!context.auth.isAuthenticated) {
      throw redirect({
        to: '/login',
        search: {
          redirect: location.href,
        },
      })
    }
  },
  component: RouteComponent,
})

function RouteComponent() {
  const router = useRouter()
  const navigate = Route.useNavigate()
  const auth = useAuth()
  const [opened, { toggle }] = useDisclosure();

  const [active, setActive] = useState('dashboard');

  const [openedPassword, { open: openPassword, close: closePassword }] = useDisclosure(false);

  const isLoading = useRouterState({ select: (s) => s.isLoading })
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [user, setUser] = useState<UserResult | null>(null);

  const init = async () => {
    setUser(await getUser());
  }

  const { t } = useTranslation();

  const data = [
    { link: '/admin', key: 'dashboard', label: t('admin.menu.dashboard'), icon: "i-mdi:monitor-dashboard" },
    { link: '/admin/sessions', key: 'sessions', label: t('admin.menu.sessions'), icon: "i-mdi:chat-processing" },
  ];

  useEffect(() => {
    init();
  }, [])

  const handleLogout = () => {
    if (window.confirm(t('admin.confirm_logout'))) {
      auth.logout().then(() => {
        router.invalidate().finally(() => {
          navigate({ to: '/login' })
        })
      })
    }
  }

  const links = data.map((item) => (
    <a
      className={classes.link}
      data-active={item.label === active || undefined}
      href={item.link}
      key={item.key}
      onClick={(event) => {
        event.preventDefault();
        setActive(item.label);
        navigate({ to: item.link as any });
      }}
    >
      <div className={`${item.icon} ${classes.linkIcon}`} />
      <span>{item.label}</span>
    </a>
  ));


  const onFormSubmit = async (evt: React.FormEvent<HTMLFormElement>) => {
    setIsSubmitting(true)
    try {
      evt.preventDefault()
      const data = new FormData(evt.currentTarget)
      const oldPasswordValue = data.get('oldPassword')
      const passwordValue = data.get('password')
      const confirmPasswordValue = data.get('confirmPassword')

      if (!oldPasswordValue || !passwordValue || !confirmPasswordValue) return;
      if (passwordValue !== confirmPasswordValue) {
        notifications.show({ color: "red", title: t('error'), message: t('admin.password_not_equal') })
      } else {
        const oldPassword = oldPasswordValue.toString();
        const password = passwordValue.toString();
        await resetPassword({ password, old_password: oldPassword });
        notifications.show({
          color: "green",
          title: t('admin.password_update_success'),
          message: t('admin.login_again')
        });
        await router.invalidate();
        auth.logout().then(() => {
          router.invalidate().finally(() => {
            navigate({ to: '/login' })
          })
        })
      }
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


  const isPasswordUpdating = isLoading || isSubmitting

  return (
    <AppShell
      header={{ height: 60 }}
      navbar={{ width: 300, breakpoint: 'sm', collapsed: { mobile: !opened } }}
      padding="md"
    >
      <AppShellHeader>
        <Group h="100%" px="md">
          <Burger opened={opened} onClick={toggle} hiddenFrom="sm" size="sm" />
          <img className={classes.logo} src={logo}></img>
          <Text>{t('title')}</Text>
        </Group>
      </AppShellHeader>
      <AppShellNavbar p="md">
        <nav className={classes.navbar}>
          <div className={classes.section}>
            <UserButton className="w-full" name={user?.name ?? ''} image='' email='' />
          </div>
          <div className={classes.navbarMain}>
            {links}
          </div>

          <div className={classes.footer}>
            <LanguageSwitcher></LanguageSwitcher>
            <a className={classes.link} onClick={openPassword}>
              <div className="i-mdi:password size-5 mr-2"></div>
              <span>{t('admin.update_password')}</span>
            </a>
            <a href="#" className={classes.link} onClick={
              (event) => {
                event.preventDefault();
                handleLogout();
              }}>
              <div className="i-material-symbols:logout size-5 mr-2"></div>
              <span>{t('logout')}</span>
            </a>
          </div>
        </nav>
      </AppShellNavbar>
      <AppShellMain>
        <Outlet />
      </AppShellMain>
      <Modal opened={openedPassword} onClose={closePassword} title={t('admin.update_password')}>
        <form className="mt-4 max-w-lg" onSubmit={onFormSubmit}>
          <PasswordInput name="oldPassword" label={t('admin.password.old_password')} placeholder={t('admin.password.old_password_hint')} required mt="md" radius="md" minLength={6} maxLength={16} />
          <PasswordInput name="password" label={t('admin.password.title')} placeholder={t('admin.password.hint')} required mt="md" radius="md" minLength={6} maxLength={16} />
          <PasswordInput name="confirmPassword" label={t('admin.password.confirm_password')} placeholder={t('admin.password.confirm_password_hint')} required mt="md" radius="md" minLength={6} maxLength={16} />
          <Button type='submit' fullWidth mt="xl" radius="md" disabled={isSubmitting}>
            {isPasswordUpdating ? t('loading') : t('admin.password.update_password')}
          </Button>
        </form>
      </Modal>
    </AppShell >

  );
}
