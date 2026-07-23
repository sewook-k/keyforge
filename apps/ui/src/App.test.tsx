import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import App from './App';

describe('KeyForge app', () => {
  it('renders the engine dashboard and all primary navigation destinations', async () => {
    render(<App />);

    expect(await screen.findByRole('heading', { name: '키 입력을 원하는 방식으로' })).toBeInTheDocument();
    expect(screen.getByLabelText('KeyForge 버전 0.1.19')).toHaveTextContent('v0.1.19');
    expect(screen.getByText('입력 엔진', { selector: '.engine-pill span' })).toBeInTheDocument();
    expect(screen.getByRole('navigation', { name: '주 메뉴' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '프로필' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '키 확인' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '자동화' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '매크로' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '창 도구' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '장치' })).toBeInTheDocument();

    await userEvent.setup().click(screen.getByRole('button', { name: '키 확인' }));
    expect(await screen.findByRole('heading', { name: '키 확인' })).toBeInTheDocument();
    expect(screen.getByRole('application', { name: '키 입력 검사 영역' })).toHaveFocus();
  });

  it('creates new profiles with global scope by default and saves them', async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole('heading', { name: '키 입력을 원하는 방식으로' });

    await user.click(screen.getByRole('button', { name: '새 프로필' }));
    const dialog = await screen.findByRole('dialog', { name: '새 프로필' });
    const globalOption = within(dialog).getByRole('radio', { name: /모든 앱과 장치에서 동작/ });
    expect(globalOption).toHaveAttribute('aria-checked', 'true');
    expect(within(dialog).getByRole('radio', { name: /특정 장치에서만 동작/ })).toBeDisabled();
    expect(within(dialog).queryByLabelText('조건 값')).not.toBeInTheDocument();

    const nameInput = within(dialog).getByLabelText('프로필 이름');
    await user.clear(nameInput);
    await user.type(nameInput, '회의용 전역 키');
    await user.click(within(dialog).getByRole('button', { name: '저장 및 적용' }));

    await waitFor(() => expect(screen.queryByRole('dialog', { name: '새 프로필' })).not.toBeInTheDocument());
    expect(await screen.findByText('회의용 전역 키 프로필을 저장하고 적용했습니다.')).toBeInTheDocument();
  });

  it('captures a keyboard chord while composing a rule', async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole('heading', { name: '키 입력을 원하는 방식으로' });
    await user.click(screen.getByRole('button', { name: '새 프로필' }));
    const profileDialog = await screen.findByRole('dialog', { name: '새 프로필' });
    await user.click(within(profileDialog).getByRole('button', { name: /규칙/ }));
    await user.click(within(profileDialog).getByRole('button', { name: '규칙 추가' }));

    const ruleDialog = await screen.findByRole('dialog', { name: '규칙 편집' });
    await user.click(within(ruleDialog).getByRole('button', { name: '입력 키 직접 누르기' }));
    const captureDialog = await screen.findByRole('dialog', { name: '입력 키 선택' });
    fireEvent.keyDown(window, { key: 'Control', code: 'ControlLeft', ctrlKey: true });
    fireEvent.keyDown(window, { key: 'k', code: 'KeyK', ctrlKey: true });

    expect(await within(captureDialog).findByText('ControlLeft + K')).toBeInTheDocument();
    await user.click(within(captureDialog).getByRole('button', { name: '이 입력 사용' }));
    expect(within(ruleDialog).getByText('ControlLeft + K')).toBeInTheDocument();

    await user.selectOptions(within(ruleDialog).getByLabelText('입력 키 목록'), 'F24');
    expect(ruleDialog.querySelector('.keycap-large')).toHaveTextContent('F24');
  });

  it('pauses and resumes the engine with a visible result notification', async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole('heading', { name: '키 입력을 원하는 방식으로' });

    await user.click(screen.getByRole('button', { name: '모두 일시정지' }));
    expect((await screen.findAllByText('모든 입력 규칙을 일시정지했습니다.')).length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: '다시 시작' })).toBeInTheDocument();
  });

  it('explains and persists whether closing keeps key mapping in the tray', async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole('heading', { name: '키 입력을 원하는 방식으로' });

    await user.click(screen.getByRole('button', { name: '설정' }));
    expect(await screen.findByText(/X는 창만 숨기고 키 매핑을 계속 실행합니다/)).toBeInTheDocument();
    const toggle = screen.getByRole('switch', { name: '알림 영역에서 계속 실행' });
    expect(toggle).toHaveAttribute('aria-checked', 'true');

    await user.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'false'));
    expect(await screen.findByText('일반 설정을 저장했습니다.')).toBeInTheDocument();
  });

  it('offers an explicit Windows login startup setting and confirms the change', async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole('button', { name: '설정' });

    await user.click(screen.getByRole('button', { name: '설정' }));
    const toggle = await screen.findByRole('switch', { name: 'Windows 로그인 시 자동 시작' });
    expect(toggle).toHaveAttribute('aria-checked', 'false');
    await user.click(toggle);
    expect(screen.getByRole('switch', { name: '시작할 때 최소화' })).toBeDisabled();

    await waitFor(() => {
      expect(toggle).toHaveAttribute('aria-checked', 'true');
      expect(screen.getByRole('switch', { name: '시작할 때 최소화' })).not.toBeDisabled();
    });
    expect(await screen.findByText('Windows 로그인 시 KeyForge를 자동 시작하도록 등록했습니다.')).toBeInTheDocument();
  });
});
