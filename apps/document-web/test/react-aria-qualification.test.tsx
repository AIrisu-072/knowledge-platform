import {
  Button,
  ComboBox,
  Dialog,
  DialogTrigger,
  FieldError,
  Form,
  Group,
  Heading,
  Input,
  Label,
  ListBox,
  ListBoxItem,
  Menu,
  MenuItem,
  MenuTrigger,
  Modal,
  Popover,
  Select,
  SelectValue,
  Tab,
  TabList,
  TabPanel,
  Tabs,
  TextField,
  Tooltip,
  TooltipTrigger,
  Tree,
  TreeItem,
  TreeItemContent,
} from 'react-aria-components';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
test('Dialog and Popover compose with fields and restore focus after Escape', async () => {
  const user = userEvent.setup();
  render(
    <DialogTrigger>
      <Button>Open version dialog</Button>
      <Modal>
        <Dialog>
          <Heading slot="title">New version</Heading>
          <Form>
            <TextField isRequired isInvalid>
              <Label>Version title</Label>
              <Input />
              <FieldError>Enter a version title.</FieldError>
            </TextField>
            <Button>Save draft</Button>
          </Form>
        </Dialog>
      </Modal>
    </DialogTrigger>,
  );

  const trigger = screen.getByRole('button', { name: 'Open version dialog' });
  await user.click(trigger);

  const dialog = screen.getByRole('dialog', { name: 'New version' });
  expect(dialog).toBeVisible();
  expect(within(dialog).getByText('Enter a version title.')).toBeVisible();
  expect(within(dialog).getByRole('textbox', { name: 'Version title' })).toHaveAttribute('aria-invalid', 'true');

  await user.keyboard('{Escape}');
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});

test('Menu supports keyboard traversal and action selection', async () => {
  const user = userEvent.setup();
  const onOpen = jest.fn();
  const onRename = jest.fn();
  render(
    <MenuTrigger>
      <Button>Document actions</Button>
      <Popover>
        <Menu aria-label="Document actions">
          <MenuItem id="open" textValue="Open" onAction={onOpen}>Open</MenuItem>
          <MenuItem id="rename" textValue="Rename" onAction={onRename}>Rename</MenuItem>
        </Menu>
      </Popover>
    </MenuTrigger>,
  );

  const trigger = screen.getByRole('button', { name: 'Document actions' });
  trigger.focus();
  await user.keyboard('{ArrowDown}');
  const menu = await screen.findByRole('menu', { name: 'Document actions' });
  expect(within(menu).getByRole('menuitem', { name: 'Open' })).toHaveFocus();
  await user.keyboard('{ArrowDown}');
  expect(within(menu).getByRole('menuitem', { name: 'Rename' })).toHaveFocus();
  await user.keyboard('{Enter}');
  await waitFor(() => expect(onRename).toHaveBeenCalledTimes(1));
  expect(onOpen).not.toHaveBeenCalled();
  expect(screen.queryByRole('menu')).not.toBeInTheDocument();
});

test('Select and ComboBox support keyboard selection and filtering', async () => {
  const user = userEvent.setup();
  render(
    <>
      <Select>
        <Label>Publication purpose</Label>
        <Button><SelectValue /></Button>
        <Popover>
          <ListBox>
            <ListBoxItem id="published">Published</ListBoxItem>
            <ListBoxItem id="authoring">Authoring</ListBoxItem>
          </ListBox>
        </Popover>
      </Select>
      <ComboBox>
        <Label>Folder</Label>
        <Group>
          <Input />
          <Button aria-label="Show folders" />
        </Group>
        <Popover>
          <ListBox>
            <ListBoxItem id="policies">Policies</ListBoxItem>
            <ListBoxItem id="templates">Templates</ListBoxItem>
          </ListBox>
        </Popover>
      </ComboBox>
    </>,
  );

  const purpose = screen.getByRole('button', { name: /Publication purpose/ });
  await user.click(purpose);
  await user.keyboard('{ArrowDown}{Enter}');
  expect(purpose).toHaveTextContent('Published');

  const folder = screen.getByRole('combobox', { name: 'Folder' });
  await user.type(folder, 'pol');
  expect(await screen.findByRole('option', { name: 'Policies' })).toBeVisible();
  expect(screen.queryByRole('option', { name: 'Templates' })).not.toBeInTheDocument();
  await user.keyboard('{ArrowDown}{Enter}');
  expect(folder).toHaveValue('Policies');
});

test('Tooltip, Tabs, and Tree expose expected keyboard-accessible semantics', async () => {
  const user = userEvent.setup();
  render(
    <>
      <TooltipTrigger delay={0} closeDelay={0}>
        <Button>Version help</Button>
        <Tooltip>Choose a published version.</Tooltip>
      </TooltipTrigger>
      <Tabs aria-label="Document sections">
        <TabList>
          <Tab id="overview">Overview</Tab>
          <Tab id="versions">Versions</Tab>
        </TabList>
        <TabPanel id="overview">Overview content</TabPanel>
        <TabPanel id="versions">Version content</TabPanel>
      </Tabs>
      <Tree aria-label="Folders" selectionMode="single" defaultExpandedKeys={['root', 'policies']}>
        <TreeItem id="root" textValue="Root">
          <TreeItemContent>Root</TreeItemContent>
          <TreeItem id="policies" textValue="Policies">
            <TreeItemContent>Policies</TreeItemContent>
            <TreeItem id="safety" textValue="Safety policy">
              <TreeItemContent>Safety policy</TreeItemContent>
            </TreeItem>
          </TreeItem>
        </TreeItem>
      </Tree>
    </>,
  );

  const help = screen.getByRole('button', { name: 'Version help' });
  help.focus();
  await waitFor(() => expect(screen.getByRole('tooltip')).toHaveTextContent('Choose a published version.'));
  expect(help).toHaveAttribute('aria-describedby');

  const overview = screen.getByRole('tab', { name: 'Overview' });
  overview.focus();
  await user.keyboard('{ArrowRight}');
  expect(screen.getByRole('tab', { name: 'Versions' })).toHaveAttribute('aria-selected', 'true');
  expect(screen.getByText('Version content')).toBeVisible();

  const root = screen.getByRole('row', { name: 'Root' });
  root.focus();
  const policies = screen.getByRole('row', { name: 'Policies' });
  const safety = screen.getByRole('row', { name: 'Safety policy' });
  expect(safety).toBeVisible();
  await user.keyboard('{ArrowDown}');
  expect(policies).toHaveAttribute('data-focused', 'true');
  await user.keyboard('{ArrowDown}');
  expect(safety).toHaveAttribute('data-focused', 'true');
});

test('Popover closes with Escape and restores its trigger focus', async () => {
  const user = userEvent.setup();
  render(
    <DialogTrigger>
      <Button>Open context popover</Button>
      <Popover>
        <Dialog>
          <Heading slot="title">Version context</Heading>
          <p>Current published version</p>
        </Dialog>
      </Popover>
    </DialogTrigger>,
  );

  const trigger = screen.getByRole('button', { name: 'Open context popover' });
  await user.click(trigger);
  expect(screen.getByRole('dialog', { name: 'Version context' })).toBeVisible();
  await user.keyboard('{Escape}');
  await waitFor(() => expect(trigger).toHaveFocus());
});

test('Tree mounts a larger folder hierarchy without losing stable item identities', () => {
  const items = Array.from({ length: 200 }, (_, index) => (
    <TreeItem key={`folder-${index}`} id={`folder-${index}`} textValue={`Folder ${index}`}>
      <TreeItemContent>Folder {index}</TreeItemContent>
    </TreeItem>
  ));
  render(
    <Tree aria-label="Large folder set" selectionMode="single">
      {items}
    </Tree>,
  );

  expect(screen.getAllByRole('row')).toHaveLength(200);
  expect(screen.getByRole('row', { name: 'Folder 199' })).toHaveAttribute('id');
});
