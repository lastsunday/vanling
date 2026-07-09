import { Center, Menu, MenuTarget, MenuDropdown, MenuItem } from "@mantine/core"
import classes from './LanguageSwitcher.module.css';
import i18n from "@/i18n/config";
import { useEffect, useState } from "react";

const items = [
  { label: "简体中文", language: "zh" },
  { label: "English", language: "en" },
]

export function LanguageSwitcher() {

  const [language, setLanguage] = useState("");

  const changeLanguage = (lng: string) => {
    i18n.changeLanguage(lng);
    setLanguage(i18n.language);
  }

  const getSelectedLanguage = (language: string) => {
    return items.filter(item => {
      return language === item.language
    });
  }

  const getSelectedLanguageDisplay = (language: string) => {
    const selectedLanguage = getSelectedLanguage(language);
    if (selectedLanguage.length > 0) {
      const item = selectedLanguage[0];
      return `${item.label}(${item.language})`;
    } else {
      return `${language}`;
    }
  }

  useEffect(() => {
    setLanguage(i18n.language);
  }, [])

  return <Menu key="language" trigger="click-hover" withinPortal closeDelay={400}>
    <MenuTarget>
      <a
        className={classes.link}
        onClick={(event) => event.preventDefault()}
      >
        <Center>
          <div className="i-mdi:language"></div>
          <div className="i-mdi:menu-down"></div>
          {getSelectedLanguageDisplay(language)}
        </Center>
      </a>
    </MenuTarget>
    <MenuDropdown className={classes.menu}>
      {
        items.map(item => {
          return <MenuItem key={item.language} onClick={() => {
            changeLanguage(item.language);
          }} className={language == item.language ? classes.menuHighline : ''} >
            {item.label}
          </MenuItem>
        })
      }
    </MenuDropdown>
  </Menu>
} 
