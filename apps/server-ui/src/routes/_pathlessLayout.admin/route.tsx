import { getUser, getVersion, resetPassword } from '@/api';
import { UserResult } from '@/data/user-result';
import { AppShell, AppShellNavbar, AppShellMain, Burger, Button, Divider, Group, Menu, MenuDropdown, MenuItem, MenuTarget, Modal, PasswordInput, ScrollArea, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { notifications } from '@mantine/notifications';
import { createFileRoute, Outlet, redirect, useRouter, useRouterState } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import logo from '../../assets/logo.svg';
import { useAuth } from '../../hooks/auth';
import { UserButton } from '../../widget/UserButton/UserButton';
import classes from './route.module.css';
import { useTranslation } from 'react-i18next';
import i18n from '@/i18n/config';

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
    { link: '/admin/devices', key: 'devices', label: t('admin.menu.devices'), icon: "i-mdi:devices" },
  ];

  const { data: version } = useQuery({
    queryKey: ['version'],
    queryFn: getVersion,
    staleTime: Infinity,
  });

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
        toggle();
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
      navbar={{ width: 300, breakpoint: 'sm', collapsed: { mobile: !opened } }}
      padding="md"
    >
      <AppShellNavbar p="md">
        <nav className={classes.navbar}>
          <div className={classes.header}>
            <Group justify="space-between" wrap="nowrap">
              <Group gap="xs" wrap="nowrap">
                <Burger opened={opened} onClick={toggle} hiddenFrom="sm" size="sm" />
                <img className={classes.logo} src={logo} />
              </Group>
              <div>
                <Text size="sm" fw={600}>{t('title')}</Text>
                <Text size="xs" c="dimmed">{t('version')}: {version ?? '...'}</Text>
              </div>
            </Group>
          </div>

          <ScrollArea className={classes.links}>
            <div className={classes.linksInner}>
              {links}
            </div>
          </ScrollArea>

          <div className={classes.footer}>
            <Menu>
              <MenuTarget>
                <UserButton className="w-full" name={user?.name ?? ''} image='' email='' />
              </MenuTarget>
              <MenuDropdown>
                <Menu.Sub>
                  <Menu.Sub.Target>
                    <Menu.Sub.Item leftSection={<div className="i-mdi:translate" />}>{t('admin.menu.language')}</Menu.Sub.Item>
                  </Menu.Sub.Target>
                  <Menu.Sub.Dropdown>
                    <MenuItem onClick={() => i18n.changeLanguage('zh')}>简体中文</MenuItem>
                    <MenuItem onClick={() => i18n.changeLanguage('en')}>English</MenuItem>
                  </Menu.Sub.Dropdown>
                </Menu.Sub>
                <Divider />
                <MenuItem leftSection={<div className="i-mdi:password" />} onClick={openPassword}>{t('admin.update_password')}</MenuItem>
                <MenuItem leftSection={<div className="i-material-symbols:logout" />} onClick={handleLogout}>{t('logout')}</MenuItem>
              </MenuDropdown>
            </Menu>
          </div>
        </nav>
      </AppShellNavbar>
      <AppShellMain>
        <Group hiddenFrom="sm" mb="md">
          <Burger opened={opened} onClick={toggle} size="sm" />
        </Group>
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
