Feature: 与机器人进行沟通
    As a 所有者
    I want 与机器人沟通 
    So that 进行连接操作

  Scenario: 获取连接地址（新设备）
    Given 含有连接所需要的基本信息
    When 所有者进行连接信息查询
    Then 所有者获得连接地址

  Scenario: 获取连接地址（已激活设备）
    Given 含有连接所需要的基本信息
    And 设备已激活
    When 所有者进行连接信息查询
    Then 所有者获得连接地址和令牌

  Scenario: 通过激活码激活设备
    Given 含有连接所需要的基本信息
    When 所有者进行连接信息查询
    And 管理员通过激活码激活设备
    Then 所有者获得连接地址和令牌