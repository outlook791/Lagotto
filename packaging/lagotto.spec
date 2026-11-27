# Pacchetto RPM di Lagotto (Fedora, openSUSE). I programmi sono già
# compilati da scripts/build-rpm.sh: qui si copiano solo al loro posto.
Name:           lagotto
Version:        %{lagotto_version}
Release:        %{lagotto_release}%{?dist}
Summary:        Recover deleted files from USB sticks, cards and disks
License:        GPL-3.0-or-later
URL:            https://github.com/outlook791/Lagotto
Source0:        lagotto-staging.tar

Requires:       sleuthkit
Requires:       testdisk
Requires:       polkit
Requires:       util-linux
Recommends:     perl-Image-ExifTool
Recommends:     smartmontools

%global debug_package %{nil}
%global __strip /bin/true

%description
Lagotto finds deleted files and brings them back, in three steps. It never
writes on the disk you recover from, and it tells you honestly how likely
recovery is. Lagotto is written with an AI assistant.

%prep
%setup -q -c -n lagotto-staging

%install
mkdir -p %{buildroot}
cp -a usr etc %{buildroot}/

%files
%license LICENSE
%{_bindir}/lagotto
%{_bindir}/lagotto-gui
%{_libexecdir}/lagotto-helper
%{_datadir}/applications/io.github.outlook791.Lagotto.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.outlook791.Lagotto.svg
%{_datadir}/metainfo/io.github.outlook791.Lagotto.metainfo.xml
%{_datadir}/polkit-1/actions/io.github.outlook791.Lagotto.policy
# Il repository di Lagotto: gli aggiornamenti arrivano con dnf e Discover.
%config(noreplace) %{_sysconfdir}/yum.repos.d/lagotto.repo
