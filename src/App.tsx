import { useEffect, useState } from 'react';
import { HashRouter, Routes, Route, Navigate, useNavigate, useLocation } from 'react-router-dom';
import { Layout, Menu, DatePicker } from 'antd';
import type { MenuProps } from 'antd';
import {
  DashboardOutlined,
  TeamOutlined,
  CalendarOutlined,
  ScanOutlined,
  SettingOutlined,
  CalculatorOutlined,
  ExportOutlined,
  DatabaseOutlined,
  FormOutlined,
  FileTextOutlined,
  AuditOutlined,
  CheckSquareOutlined,
  WalletOutlined,
  BankOutlined,
  BarChartOutlined,
  AppstoreOutlined,
  LockOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';

import Dashboard from '@/pages/Dashboard';
import Employees from '@/pages/Employees';
import Attendance from '@/pages/Attendance';
import OcrCenter from '@/pages/OcrCenter';
import Invoices from '@/pages/Invoices';
import SalaryRules from '@/pages/SalaryRules';
import SalaryCalculate from '@/pages/SalaryCalculate';
import ExportCenter from '@/pages/ExportCenter';
import PunchCard from '@/pages/PunchCard';
import MonthClose from '@/pages/MonthClose';
import OperationLogs from '@/pages/OperationLogs';
import Reimbursements from '@/pages/Reimbursements';
import FinancialAnalysis from '@/pages/FinancialAnalysis';
import DataSafety from '@/pages/DataSafety';
import Payments from '@/pages/Payments';
import BankTransactions from '@/pages/BankTransactions';
import SecurityCenter from '@/pages/SecurityCenter';

const { Sider, Header, Content } = Layout;

const menuItems: MenuProps['items'] = [
  {
    key: 'workbench',
    label: '工作台',
    icon: <DashboardOutlined />,
    children: [
      { key: '/', label: '首页仪表盘', icon: <DashboardOutlined /> },
      { key: '/month-close', label: '月结工作台', icon: <CheckSquareOutlined /> },
      { key: '/financial-analysis', label: '财务分析', icon: <BarChartOutlined /> },
    ],
  },
  {
    key: 'people-attendance',
    label: '人员考勤',
    icon: <TeamOutlined />,
    children: [
      { key: '/employees', label: '员工管理', icon: <TeamOutlined /> },
      { key: '/attendance', label: '考勤管理', icon: <CalendarOutlined /> },
      { key: '/punch-card', label: '打卡表管理', icon: <FormOutlined /> },
      { key: '/ocr', label: 'OCR识别中心', icon: <ScanOutlined /> },
    ],
  },
  {
    key: 'salary-payroll',
    label: '薪酬核算',
    icon: <CalculatorOutlined />,
    children: [
      { key: '/salary', label: '工资计算', icon: <CalculatorOutlined /> },
      { key: '/payments', label: '付款批次', icon: <WalletOutlined /> },
      { key: '/bank-transactions', label: '银行流水', icon: <BankOutlined /> },
    ],
  },
  {
    key: 'invoice-reimbursement',
    label: '票据报销',
    icon: <WalletOutlined />,
    children: [
      { key: '/invoices', label: '发票管理', icon: <FileTextOutlined /> },
      { key: '/reimbursements', label: '报销管理', icon: <WalletOutlined /> },
    ],
  },
  {
    key: 'output-audit',
    label: '输出审计',
    icon: <AppstoreOutlined />,
    children: [
      { key: '/export', label: '导出中心', icon: <ExportOutlined /> },
      { key: '/logs', label: '操作日志', icon: <AuditOutlined /> },
      { key: '/data-safety', label: '数据安全', icon: <DatabaseOutlined /> },
    ],
  },
  { key: '/rules', label: '系统设置', icon: <SettingOutlined /> },
  { key: '/security', label: '安全中心', icon: <LockOutlined /> },
];

const menuPathToGroupKey = new Map<string, string>();
for (const item of menuItems) {
  if (!item || !('children' in item) || !item.children) continue;
  for (const child of item.children) {
    if (!child || !('key' in child) || typeof child.key !== 'string') continue;
    menuPathToGroupKey.set(child.key, String(item.key));
  }
}

const defaultOpenKeys = ['workbench'];

const AppLayout: React.FC = () => {
  const [collapsed, setCollapsed] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const [globalMonth, setGlobalMonth] = useState<Dayjs>(dayjs());
  const [openKeys, setOpenKeys] = useState<string[]>(defaultOpenKeys);

  useEffect(() => {
    const groupKey = menuPathToGroupKey.get(location.pathname);
    if (!groupKey) return;
    setOpenKeys((keys) => (
      keys.includes(groupKey) ? keys : [...keys, groupKey]
    ));
  }, [location.pathname]);

  const handleOpenChange: MenuProps['onOpenChange'] = (keys) => {
    const nextOpenKeys = keys as string[];
    if (collapsed) {
      setCollapsed(false);
      setOpenKeys((currentKeys) => {
        const openedKey = nextOpenKeys.find((key) => !currentKeys.includes(key)) ?? nextOpenKeys[0];
        if (!openedKey || currentKeys.includes(openedKey)) return currentKeys;
        return [...currentKeys, openedKey];
      });
      return;
    }
    setOpenKeys(nextOpenKeys);
  };

  const handleMenuClick: MenuProps['onClick'] = ({ key }) => {
    navigate(key);
  };

  return (
    <Layout className="app-layout">
      <Sider
        className="app-sider"
        collapsible
        collapsed={collapsed}
        onCollapse={setCollapsed}
        width={220}
        theme="dark"
      >
        <div className={`logo ${collapsed ? 'collapsed' : ''}`}>
          <span>{collapsed ? '工资' : '工资核算助手'}</span>
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[location.pathname]}
          openKeys={collapsed ? [] : openKeys}
          items={menuItems}
          onOpenChange={handleOpenChange}
          onClick={handleMenuClick}
          style={{ borderRight: 0 }}
        />
      </Sider>

      <div className={`app-content-wrapper ${collapsed ? 'collapsed' : ''}`}>
        <Header className="app-header">
          <div className="app-header-left" />
          <div className="app-header-right">
            <span style={{ color: '#666', fontSize: 14 }}>当前月份：</span>
            <DatePicker
              picker="month"
              value={globalMonth}
              onChange={(d) => d && setGlobalMonth(d)}
              allowClear={false}
              style={{ width: 160 }}
            />
          </div>
        </Header>

        <Content className="app-main">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/employees" element={<Employees />} />
            <Route path="/attendance" element={<Attendance />} />
            <Route path="/punch-card" element={<PunchCard />} />
            <Route path="/ocr" element={<OcrCenter />} />
            <Route path="/invoices" element={<Invoices />} />
            <Route path="/reimbursements" element={<Reimbursements />} />
            <Route path="/month-close" element={<MonthClose />} />
            <Route path="/financial-analysis" element={<FinancialAnalysis />} />
            <Route path="/rules" element={<SalaryRules />} />
            <Route path="/salary" element={<SalaryCalculate />} />
            <Route path="/payments" element={<Payments />} />
            <Route path="/bank-transactions" element={<BankTransactions />} />
            <Route path="/export" element={<ExportCenter />} />
            <Route path="/logs" element={<OperationLogs />} />
            <Route path="/data-safety" element={<DataSafety />} />
            <Route path="/security" element={<SecurityCenter />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </Content>
      </div>
    </Layout>
  );
};

function App() {
  return (
    <HashRouter>
      <AppLayout />
    </HashRouter>
  );
}

export default App;
